package galileo_test

import (
	"context"
	"errors"
	"fmt"
	"io/fs"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/stretchr/testify/require"
	"go.opentelemetry.io/otel/attribute"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"
	"go.opentelemetry.io/otel/trace"

	galileo "github.com/Serious-Sam97/galileo/sdk/go"
)

func recorded(t *testing.T) (*galileo.Telemetry, *tracetest.SpanRecorder) {
	t.Helper()
	rec := tracetest.NewSpanRecorder()
	tel, err := galileo.Init(context.Background(), galileo.Config{Service: "shop-api", Env: "test", CallSites: true, SpanProcessor: rec})
	require.NoError(t, err)
	t.Cleanup(func() { _ = tel.Shutdown(context.Background()) })
	return tel, rec
}

func attrsOf(kvs []attribute.KeyValue) map[attribute.Key]attribute.Value {
	out := map[attribute.Key]attribute.Value{}
	for _, kv := range kvs {
		out[kv.Key] = kv.Value
	}
	return out
}

func byName(t *testing.T, spans []sdktrace.ReadOnlySpan, name string) sdktrace.ReadOnlySpan {
	t.Helper()
	for _, s := range spans {
		if s.Name() == name {
			return s
		}
	}
	names := make([]string, 0, len(spans))
	for _, s := range spans {
		names = append(names, s.Name())
	}
	t.Fatalf("no span %q in %v", name, names)
	return nil
}

func TestTheServerSpanIsNamedAfterTheRouteAndCarriesIdentity(t *testing.T) {
	tel, rec := recorded(t)
	mux := http.NewServeMux()
	mux.HandleFunc("GET /orders/{id}", func(w http.ResponseWriter, r *http.Request) {
		// a query inside the handler inherits the identity from the context
		_, span := galileo.StartQuery(r.Context(), "postgresql", "SELECT * FROM orders WHERE id = $1")
		galileo.EndQuery(span, nil)
		w.WriteHeader(http.StatusCreated)
		_, _ = w.Write([]byte(`{"ok":true}`))
	})
	identify := galileo.Identify(func(*http.Request) (galileo.Identity, bool) {
		return galileo.Identity{UserID: "42", Email: "vet@aumiau.test", TenantID: "7", TenantName: "Au Miau"}, true
	})
	h := tel.Middleware(galileo.WithServeMux(mux))(identify(mux))

	req := httptest.NewRequest(http.MethodGet, "/orders/12", nil)
	req.Header.Set("X-Tenant-ID", "7")
	w := httptest.NewRecorder()
	h.ServeHTTP(w, req)
	require.Equal(t, http.StatusCreated, w.Code)

	server := byName(t, rec.Ended(), "GET /orders/{id}")
	require.Equal(t, trace.SpanKindServer, server.SpanKind())
	a := attrsOf(server.Attributes())
	require.Equal(t, "/orders/{id}", a["http.route"].AsString())
	require.Equal(t, "/orders/12", a["url.path"].AsString())
	require.Equal(t, "7", a["http.request.header.x-tenant-id"].AsString())
	require.EqualValues(t, 201, a["http.response.status_code"].AsInt64())
	require.EqualValues(t, 201, a["http.status_code"].AsInt64(), "legacy spelling survives")
	require.EqualValues(t, 11, a["http.response.body.size"].AsInt64())
	require.Equal(t, "42", a["user.id"].AsString(), "identity set after the span opened still lands on it")
	require.Equal(t, "7", a["tenant.id"].AsString())
	require.Equal(t, "Au Miau", a["tenant.name"].AsString())
	require.Equal(t, server.SpanContext().TraceID().String(), w.Header().Get("x-galileo-trace-id"))

	query := byName(t, rec.Ended(), "SELECT orders")
	qa := attrsOf(query.Attributes())
	require.Equal(t, "42", qa["user.id"].AsString(), "child spans carry identity")
	require.Equal(t, server.SpanContext().SpanID(), query.Parent().SpanID())
}

func TestWrappingTheMuxDirectlyNeedsNoOption(t *testing.T) {
	tel, rec := recorded(t)
	mux := http.NewServeMux()
	mux.HandleFunc("POST /pets/{id}/vaccines", func(http.ResponseWriter, *http.Request) {})
	tel.Middleware()(mux).ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodPost, "/pets/3/vaccines", nil))
	require.Equal(t, "POST /pets/{id}/vaccines", rec.Ended()[0].Name())
}

func TestAnIncomingTraceparentIsContinued(t *testing.T) {
	tel, rec := recorded(t)
	h := tel.Middleware()(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {}))
	req := httptest.NewRequest(http.MethodGet, "/", nil)
	req.Header.Set("traceparent", "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01")
	h.ServeHTTP(httptest.NewRecorder(), req)
	span := rec.Ended()[0]
	require.Equal(t, "4bf92f3577b34da6a3ce929d0e0e4736", span.SpanContext().TraceID().String())
	require.Equal(t, "00f067aa0ba902b7", span.Parent().SpanID().String())
}

func TestAnUnrecoveredPanicClosesTheSpanAsA500AndKeepsPanicking(t *testing.T) {
	tel, rec := recorded(t)
	h := tel.Middleware()(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { panic("kaboom") }))
	require.PanicsWithValue(t, "kaboom", func() {
		h.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "/boom", nil))
	})
	spans := rec.Ended()
	require.Len(t, spans, 1)
	require.EqualValues(t, 500, attrsOf(spans[0].Attributes())["http.response.status_code"].AsInt64())
	require.Len(t, spans[0].Events(), 1)
	ev := attrsOf(spans[0].Events()[0].Attributes)
	require.Equal(t, "panic", ev["exception.type"].AsString(), "typed so panics group by message, not by Go type")
	require.Equal(t, "kaboom", ev["exception.message"].AsString())
	require.Contains(t, ev["exception.stacktrace"].AsString(), "goroutine")
}

func TestAPanicRecoveredInsideIsRecordedOnce(t *testing.T) {
	tel, rec := recorded(t)
	recoverer := func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			defer func() {
				if p := recover(); p != nil {
					galileo.RecordPanic(r.Context(), p)
					w.WriteHeader(http.StatusInternalServerError)
				}
			}()
			next.ServeHTTP(w, r)
		})
	}
	h := tel.Middleware()(recoverer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { panic("kaboom") })))
	w := httptest.NewRecorder()
	h.ServeHTTP(w, httptest.NewRequest(http.MethodGet, "/boom", nil))
	require.Equal(t, http.StatusInternalServerError, w.Code)
	require.Len(t, rec.Ended()[0].Events(), 1)
}

func TestWithoutAnEndpointNothingIsRecordedAndNothingBreaks(t *testing.T) {
	tel, err := galileo.Init(context.Background(), galileo.Config{Service: "shop-api"})
	require.NoError(t, err)
	require.False(t, tel.Enabled())
	t.Cleanup(func() { _ = tel.Shutdown(context.Background()) })
	h := tel.Middleware()(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		require.False(t, trace.SpanFromContext(r.Context()).IsRecording())
		w.WriteHeader(http.StatusNoContent)
	}))
	w := httptest.NewRecorder()
	h.ServeHTTP(w, httptest.NewRequest(http.MethodGet, "/", nil))
	require.Equal(t, http.StatusNoContent, w.Code)
}

func TestQueriesAreNamedByOperationAndTableAndPointAtTheCaller(t *testing.T) {
	tel, rec := recorded(t)
	ctx, parent := tel.Tracer().Start(context.Background(), "GET /x")
	long := "SELECT * FROM pets WHERE " + strings.Repeat("é", 2000)
	_, span := galileo.StartQuery(ctx, "postgresql", long)
	galileo.EndQuery(span, errors.New("no rows"), func(err error) bool { return err.Error() == "no rows" })
	parent.End()

	q := byName(t, rec.Ended(), "SELECT pets")
	a := attrsOf(q.Attributes())
	require.Equal(t, "postgresql", a["db.system"].AsString(), "legacy spelling")
	require.Equal(t, "postgresql", a["db.system.name"].AsString(), "current spelling")
	require.Equal(t, "pets", a["db.collection.name"].AsString())
	require.LessOrEqual(t, len(a["db.query.text"].AsString()), galileo.MaxStatement+len("…"))
	require.True(t, strings.HasSuffix(a["db.query.text"].AsString(), "…"))
	require.Equal(t, "TestQueriesAreNamedByOperationAndTableAndPointAtTheCaller", a["code.function.name"].AsString())
	require.True(t, strings.HasSuffix(a["code.file.path"].AsString(), "galileo_test.go"))
	require.Positive(t, a["code.line.number"].AsInt64())
	require.Empty(t, q.Events(), "an ignored error is an answer, not a failure")
}

func TestNoQuerySpanWithoutARecordingParent(t *testing.T) {
	_, rec := recorded(t)
	_, span := galileo.StartQuery(context.Background(), "postgresql", "SELECT 1")
	galileo.EndQuery(span, nil)
	require.Empty(t, rec.Ended())
}

func TestTransportPropagatesTheTraceToTheCalledService(t *testing.T) {
	tel, rec := recorded(t)
	var got string
	srv := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) { got = r.Header.Get("traceparent") }))
	defer srv.Close()

	ctx, parent := tel.Tracer().Start(context.Background(), "GET /checkout")
	req, _ := http.NewRequestWithContext(ctx, http.MethodPost, srv.URL+"/invoices?token=secret", nil)
	resp, err := (&http.Client{Transport: galileo.Transport(nil)}).Do(req)
	require.NoError(t, err)
	_ = resp.Body.Close()
	parent.End()

	client := rec.Ended()[0]
	require.Equal(t, trace.SpanKindClient, client.SpanKind())
	require.Contains(t, got, client.SpanContext().TraceID().String())
	require.Contains(t, got, client.SpanContext().SpanID().String(), "the callee's parent is the client span")
	require.NotContains(t, attrsOf(client.Attributes())["url.full"].AsString(), "secret")
}

func TestGatewayHeadersCarryTheTraceAndTheIdentity(t *testing.T) {
	tel, _ := recorded(t)
	ctx, span := tel.Tracer().Start(context.Background(), "POST /notes")
	defer span.End()
	ctx = galileo.WithIdentity(ctx, galileo.Identity{UserID: "42", TenantID: "7"})
	h := galileo.GatewayHeaders(ctx, nil)
	require.Contains(t, h.Get("traceparent"), span.SpanContext().TraceID().String())
	require.Equal(t, "42", h.Get("x-galileo-user-id"))
	require.Equal(t, "7", h.Get("x-galileo-tenant-id"))
}

func TestJobsRecordTheInnermostErrorType(t *testing.T) {
	_, rec := recorded(t)
	err := galileo.Job(context.Background(), "reminders", func(ctx context.Context) error {
		return fmt.Errorf("clinic 7: %w", &fs.PathError{Op: "open", Path: "x", Err: fs.ErrNotExist})
	})
	require.Error(t, err)
	job := byName(t, rec.Ended(), "job reminders")
	require.Equal(t, trace.SpanKindConsumer, job.SpanKind())
	ev := attrsOf(job.Events()[0].Attributes)
	require.Equal(t, "*errors.errorString", ev["exception.type"].AsString(), "unwrapped to the root cause")
}

func TestAPanickingJobIsRecordedOnceAndKeepsPanicking(t *testing.T) {
	_, rec := recorded(t)
	require.PanicsWithValue(t, "nil clinic", func() {
		_ = galileo.Job(context.Background(), "reminders", func(context.Context) error { panic("nil clinic") })
	})
	job := byName(t, rec.Ended(), "job reminders")
	require.Len(t, job.Events(), 1)
	require.Equal(t, "panic", attrsOf(job.Events()[0].Attributes)["exception.type"].AsString())
}

func chargeCard(ctx context.Context) { galileo.RecordError(ctx, errors.New("card declined")) }

func loadClinic() { var m map[string]int; m["x"]++ } // panics: assignment to nil map

func TestExceptionsNameTheirCulpritSoIssuesGroupByIt(t *testing.T) {
	tel, rec := recorded(t)
	ctx, span := tel.Tracer().Start(context.Background(), "POST /pay")
	chargeCard(ctx)
	span.End()
	require.Equal(t, "chargeCard", attrsOf(rec.Ended()[0].Attributes())["code.function.name"].AsString())

	// the panicking function, not the recovery middleware that caught it
	h := tel.Middleware()(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer func() {
			if p := recover(); p != nil {
				galileo.RecordPanic(r.Context(), p)
				w.WriteHeader(http.StatusInternalServerError)
			}
		}()
		loadClinic()
	}))
	h.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, "/clinic", nil))
	a := attrsOf(rec.Ended()[1].Attributes())
	require.Equal(t, "loadClinic", a["code.function.name"].AsString())
	require.Equal(t, "github.com/Serious-Sam97/galileo/sdk/go_test", a["code.namespace"].AsString())
}

func TestConfigFromEnv(t *testing.T) {
	t.Setenv("GALILEO_OTLP_ENDPOINT", "https://galileo-api.example.com/otlp")
	t.Setenv("GALILEO_API_KEY", "glk_x")
	t.Setenv("OTEL_SERVICE_NAME", "shop-api")
	t.Setenv("APP_VERSION", "1.4.2")
	t.Setenv("GALILEO_SAMPLE_RATIO", "0.2")
	c := galileo.ConfigFromEnv()
	require.Equal(t, "https://galileo-api.example.com/otlp", c.Endpoint)
	require.Equal(t, "glk_x", c.APIKey)
	require.Equal(t, "shop-api", c.Service)
	require.Equal(t, "1.4.2", c.Release)
	require.InDelta(t, 0.2, c.SampleRatio, 1e-9)
	require.True(t, c.CallSites)
	require.True(t, c.Logs)
	require.Equal(t, slog.LevelInfo, c.LogLevel)
	require.True(t, c.Metrics)
	require.Equal(t, time.Minute, c.MetricsInterval)

	t.Setenv("GALILEO_LOGS", "0")
	t.Setenv("GALILEO_LOG_LEVEL", "warn")
	c = galileo.ConfigFromEnv()
	require.False(t, c.Logs)
	require.Equal(t, slog.LevelWarn, c.LogLevel)

	t.Setenv("GALILEO_METRICS", "0")
	t.Setenv("GALILEO_METRICS_INTERVAL", "15")
	c = galileo.ConfigFromEnv()
	require.False(t, c.Metrics)
	require.Equal(t, 15*time.Second, c.MetricsInterval)
}
