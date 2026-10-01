package galileo_test

import (
	"bytes"
	"context"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"testing"

	"github.com/stretchr/testify/require"
	"go.opentelemetry.io/otel/attribute"
	otellog "go.opentelemetry.io/otel/log"
	sdklog "go.opentelemetry.io/otel/sdk/log"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"
	"go.opentelemetry.io/otel/trace"

	galileo "github.com/Serious-Sam97/galileo/sdk/go"
)

// logRecorder keeps every emitted record in memory.
type logRecorder struct {
	mu      sync.Mutex
	records []sdklog.Record
}

func (r *logRecorder) OnEmit(_ context.Context, rec *sdklog.Record) error {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.records = append(r.records, rec.Clone())
	return nil
}
func (r *logRecorder) Enabled(context.Context, sdklog.EnabledParameters) bool { return true }
func (r *logRecorder) Shutdown(context.Context) error                         { return nil }
func (r *logRecorder) ForceFlush(context.Context) error                       { return nil }

func (r *logRecorder) all() []sdklog.Record {
	r.mu.Lock()
	defer r.mu.Unlock()
	return append([]sdklog.Record(nil), r.records...)
}

func logAttrs(rec sdklog.Record) map[string]string {
	out := map[string]string{}
	rec.WalkAttributes(func(kv attribute.KeyValue) bool {
		out[string(kv.Key)] = kv.Value.Emit()
		return true
	})
	return out
}

func recordedLogs(t *testing.T, level slog.Level) (*galileo.Telemetry, *tracetest.SpanRecorder, *logRecorder) {
	t.Helper()
	spans, logs := tracetest.NewSpanRecorder(), &logRecorder{}
	tel, err := galileo.Init(context.Background(), galileo.Config{Service: "shop-api", SpanProcessor: spans, Logs: true, LogLevel: level, LogProcessor: logs})
	require.NoError(t, err)
	t.Cleanup(func() { _ = tel.Shutdown(context.Background()) })
	return tel, spans, logs
}

func TestLogsCarryTheRequestTraceAndIdentity(t *testing.T) {
	tel, spans, logs := recordedLogs(t, slog.LevelInfo)
	var local bytes.Buffer
	logger := slog.New(tel.SlogHandler(slog.NewTextHandler(&local, nil))).WithGroup("order")
	h := tel.Middleware()(galileo.Identify(func(*http.Request) (galileo.Identity, bool) {
		return galileo.Identity{UserID: "42", TenantID: "clinic-7"}, true
	})(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		logger.InfoContext(r.Context(), "order placed", "id", 9)
	})))
	h.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "/orders", nil))

	got := logs.all()
	require.Len(t, got, 1)
	require.Equal(t, "order placed", got[0].Body().AsString())
	require.Equal(t, otellog.SeverityInfo, got[0].Severity())
	require.Equal(t, spans.Ended()[0].SpanContext().TraceID(), got[0].TraceID())
	a := logAttrs(got[0])
	require.Equal(t, "42", a["user.id"])
	require.Equal(t, "clinic-7", a["tenant.id"])
	require.Contains(t, local.String(), "order placed", "the local handler still gets the line")
}

func TestLogLevelAppliesToGalileoOnly(t *testing.T) {
	tel, _, logs := recordedLogs(t, slog.LevelWarn)
	var local bytes.Buffer
	logger := slog.New(tel.SlogHandler(slog.NewTextHandler(&local, &slog.HandlerOptions{Level: slog.LevelDebug})))
	logger.Debug("cache warm")
	logger.Error("payment failed")

	got := logs.all()
	require.Len(t, got, 1)
	require.Equal(t, "payment failed", got[0].Body().AsString())
	require.Equal(t, 2, strings.Count(local.String(), "\n"))
}

func TestWithoutLogExportTheLocalHandlerIsReturnedAsIs(t *testing.T) {
	tel, err := galileo.Init(context.Background(), galileo.Config{Service: "shop-api"})
	require.NoError(t, err)
	local := slog.NewTextHandler(&bytes.Buffer{}, nil)
	require.Equal(t, slog.Handler(local), tel.SlogHandler(local))
	slog.New(tel.SlogHandler(nil)).InfoContext(trace.ContextWithSpanContext(context.Background(), trace.SpanContext{}), "dropped")
}
