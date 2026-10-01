package galileo_test

import (
	"context"
	"errors"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/stretchr/testify/require"
	"go.opentelemetry.io/otel/attribute"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"

	galileo "github.com/Serious-Sam97/galileo/sdk/go"
)

// metered starts the SDK with metrics read on demand and traces that keep almost nothing, so the
// tests show metrics do not depend on sampling.
func metered(t *testing.T) (*galileo.Telemetry, *sdkmetric.ManualReader, *tracetest.SpanRecorder) {
	t.Helper()
	reader, spans := sdkmetric.NewManualReader(), tracetest.NewSpanRecorder()
	tel, err := galileo.Init(context.Background(), galileo.Config{Service: "shop-api", SpanProcessor: spans, SampleRatio: 1e-12, Metrics: true, MetricReader: reader})
	require.NoError(t, err)
	t.Cleanup(func() { _ = tel.Shutdown(context.Background()) })
	return tel, reader, spans
}

func collect(t *testing.T, reader *sdkmetric.ManualReader) map[string]metricdata.Metrics {
	t.Helper()
	var rm metricdata.ResourceMetrics
	require.NoError(t, reader.Collect(context.Background(), &rm))
	out := map[string]metricdata.Metrics{}
	for _, sm := range rm.ScopeMetrics {
		for _, m := range sm.Metrics {
			out[m.Name] = m
		}
	}
	return out
}

func histogramPoints(t *testing.T, m metricdata.Metrics) []metricdata.HistogramDataPoint[float64] {
	t.Helper()
	h, ok := m.Data.(metricdata.Histogram[float64])
	require.True(t, ok, "%s is %T", m.Name, m.Data)
	return h.DataPoints
}

func setOf(kvs ...attribute.KeyValue) attribute.Set { return attribute.NewSet(kvs...) }

func TestEveryRequestIsMeasuredEvenWhenItsTraceIsSampledOut(t *testing.T) {
	tel, reader, spans := metered(t)
	mux := http.NewServeMux()
	mux.HandleFunc("GET /orders/{id}", func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(http.StatusTeapot) })
	h := tel.Middleware(galileo.WithServeMux(mux))(galileo.Identify(func(*http.Request) (galileo.Identity, bool) {
		return galileo.Identity{UserID: "42", TenantID: "clinic-7"}, true
	})(mux))
	for range 3 {
		h.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "/orders/9", nil))
	}
	h.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "/wp-login.php", nil))
	require.Empty(t, spans.Ended(), "the traces were sampled out")

	got := collect(t, reader)
	pts := histogramPoints(t, got[galileo.MetricServerDuration])
	require.Len(t, pts, 2)
	byAttrs := map[attribute.Set]uint64{}
	for _, p := range pts {
		byAttrs[p.Attributes] = p.Count
	}
	require.Equal(t, uint64(3), byAttrs[setOf(
		attribute.String("http.method", "GET"), attribute.Int("http.status_code", 418),
		attribute.String("http.route", "/orders/{id}"), attribute.String("tenant.id", "clinic-7"))])
	require.Equal(t, uint64(1), byAttrs[setOf(
		attribute.String("http.method", "GET"), attribute.Int("http.status_code", 404),
		attribute.String("tenant.id", "clinic-7"))], "an unmatched path adds no route series")

	requests := got[galileo.MetricServerRequests].Data.(metricdata.Sum[int64])
	var total int64
	for _, p := range requests.DataPoints {
		total += p.Value
	}
	require.Equal(t, int64(4), total)
	active := got[galileo.MetricServerActive].Data.(metricdata.Sum[int64])
	require.Equal(t, int64(0), active.DataPoints[0].Value, "nothing left in flight")
}

func TestStatementsAreTimedWithoutASpanAndOnlyRealFailuresCarryAnErrorType(t *testing.T) {
	_, reader, spans := metered(t)
	_, span := galileo.StartQuery(context.Background(), "postgresql", "SELECT * FROM pets WHERE id = $1")
	galileo.EndQuery(span, errors.New("no rows"), func(err error) bool { return err.Error() == "no rows" })
	_, span = galileo.StartQuery(context.Background(), "postgresql", "UPDATE pets SET name = $1")
	galileo.EndQuery(span, context.DeadlineExceeded)
	require.Empty(t, spans.Ended())

	pts := histogramPoints(t, collect(t, reader)[galileo.MetricDBDuration])
	require.Len(t, pts, 2)
	sets := []attribute.Set{pts[0].Attributes, pts[1].Attributes}
	require.Contains(t, sets, setOf(
		attribute.String("db.system.name", "postgresql"), attribute.String("db.operation.name", "SELECT"),
		attribute.String("db.collection.name", "pets")))
	require.Contains(t, sets, setOf(
		attribute.String("db.system.name", "postgresql"), attribute.String("db.operation.name", "UPDATE"),
		attribute.String("db.collection.name", "pets"), attribute.String("error.type", "context.deadlineExceededError")))
}

func TestJobsAndOutgoingCallsAreTimed(t *testing.T) {
	_, reader, _ := metered(t)
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { w.WriteHeader(http.StatusBadGateway) }))
	defer srv.Close()

	_ = galileo.Job(context.Background(), "send-reminders", func(ctx context.Context) error {
		req, _ := http.NewRequestWithContext(ctx, http.MethodPost, srv.URL+"/sms", nil)
		resp, err := (&http.Client{Transport: galileo.Transport(nil)}).Do(req)
		require.NoError(t, err)
		_ = resp.Body.Close()
		return errors.New("provider down")
	})

	got := collect(t, reader)
	job := histogramPoints(t, got[galileo.MetricJobDuration])
	require.Len(t, job, 1)
	require.Equal(t, setOf(attribute.String("job.name", "send-reminders"), attribute.String("error.type", "*errors.errorString")), job[0].Attributes)
	call := histogramPoints(t, got[galileo.MetricClientDuration])
	require.Len(t, call, 1)
	status, _ := call[0].Attributes.Value("http.status_code")
	require.Equal(t, int64(502), status.AsInt64())
}

func TestRuntimeAndHostMetricsAreReported(t *testing.T) {
	_, reader, _ := metered(t)
	got := collect(t, reader)
	for _, name := range []string{"go.goroutine.count", "go.memory.used", "process.cpu.time", "system.memory.usage"} {
		require.Contains(t, got, name)
	}
}

func TestOwnInstrumentsUseTheSDKMeter(t *testing.T) {
	_, reader, _ := metered(t)
	orders, err := galileo.Meter().Int64Counter("orders.placed")
	require.NoError(t, err)
	orders.Add(context.Background(), 2)
	require.Contains(t, collect(t, reader), "orders.placed")
}

func TestWithoutMetricsNothingIsTimed(t *testing.T) {
	tel, err := galileo.Init(context.Background(), galileo.Config{Service: "shop-api"})
	require.NoError(t, err)
	t.Cleanup(func() { _ = tel.Shutdown(context.Background()) })
	require.False(t, galileo.MetricsEnabled())
	_, span := galileo.StartQuery(context.Background(), "postgresql", "SELECT 1")
	galileo.EndQuery(span, nil)
}
