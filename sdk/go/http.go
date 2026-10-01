package galileo

import (
	"context"
	"fmt"
	"net/http"
	"strconv"
	"strings"
	"time"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/codes"
	"go.opentelemetry.io/otel/metric"
	"go.opentelemetry.io/otel/propagation"
	"go.opentelemetry.io/otel/trace"
)

// MiddlewareOption configures [Telemetry.Middleware].
type MiddlewareOption func(*middlewareConfig)

type middlewareConfig struct {
	route   func(*http.Request) string
	headers []string
}

// WithRoute tells the middleware how to read the matched route template after the handler ran
// (see the chiroute package for chi). The default reads http.Request.Pattern, which net/http's
// ServeMux sets only when the middleware wraps the mux directly; with other middleware in
// between use [WithServeMux].
func WithRoute(route func(*http.Request) string) MiddlewareOption {
	return func(c *middlewareConfig) { c.route = route }
}

// WithServeMux names spans after the pattern mux matches for the request, however many
// middlewares sit between this one and the mux.
//
//	handler := tel.Middleware(galileo.WithServeMux(mux))(auth(identify(mux)))
func WithServeMux(mux *http.ServeMux) MiddlewareOption {
	return WithRoute(func(r *http.Request) string {
		_, pattern := mux.Handler(r)
		return stdlibPattern(&http.Request{Pattern: pattern})
	})
}

// WithCapturedHeaders records these request headers as http.request.header.<name>. The default
// is x-tenant-id and x-request-id.
func WithCapturedHeaders(names ...string) MiddlewareOption {
	return func(c *middlewareConfig) { c.headers = names }
}

// Middleware opens one server span per request. Put it as far out as possible — before
// authentication and before your panic recovery — so identity can be added to it later and a
// panic still closes it with its 500.
//
// The span continues an incoming traceparent (browser, other services), is renamed to the route
// template once routing is done ("GET /orders/{id}", so a thousand visits are one row), carries
// http.* attributes and an error status on 5xx, and the response gets x-galileo-trace-id.
//
// Every request, sampled or not, is also counted in http.server.request.duration,
// http.server.requests and http.server.active_requests (route, method, status, tenant.id).
func (t *Telemetry) Middleware(opts ...MiddlewareOption) func(http.Handler) http.Handler {
	cfg := middlewareConfig{route: stdlibPattern, headers: []string{"x-tenant-id", "x-request-id"}}
	for _, o := range opts {
		o(&cfg)
	}
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			ctx := otel.GetTextMapPropagator().Extract(r.Context(), propagation.HeaderCarrier(r.Header))
			ctx, span := t.Tracer().Start(ctx, r.Method+" "+r.URL.Path,
				trace.WithSpanKind(trace.SpanKindServer),
				trace.WithAttributes(
					attribute.String("http.request.method", r.Method),
					attribute.String("url.path", r.URL.Path),
					attribute.String("server.address", r.Host),
					attribute.String("user_agent.original", r.UserAgent()),
				))
			defer span.End()
			recording, in := span.IsRecording(), active.Load()
			if !recording && in == nil {
				next.ServeHTTP(w, r.WithContext(ctx))
				return
			}
			var who *Identity
			if in != nil {
				who = new(Identity)
				ctx = context.WithValue(ctx, identitySlotKey{}, who)
				in.serverActive.Add(ctx, 1, metric.WithAttributes(attribute.String("http.method", r.Method)))
			}
			start := time.Now()
			if recording {
				for _, h := range cfg.headers {
					if v := r.Header.Get(h); v != "" {
						span.SetAttributes(attribute.String("http.request.header."+strings.ToLower(h), v))
					}
				}
				w.Header().Set("x-galileo-trace-id", span.SpanContext().TraceID().String())
			}
			rec := &statusRecorder{ResponseWriter: w}
			inner := r.WithContext(ctx)

			defer func() {
				pattern := cfg.route(inner)
				status := rec.status
				p := recover()
				if p != nil {
					// nobody recovered inside: record it, answer 500 on the span, keep panicking
					status = http.StatusInternalServerError
					RecordPanic(ctx, p)
				}
				if status == 0 {
					status = http.StatusOK
				}
				if in != nil {
					in.recordServer(ctx, r.Method, pattern, status, who, time.Since(start))
				}
				if !recording {
					if p != nil {
						panic(p)
					}
					return
				}
				if pattern != "" {
					span.SetName(r.Method + " " + pattern)
					span.SetAttributes(attribute.String("http.route", pattern))
				}
				if id := w.Header().Get("X-Request-ID"); id != "" {
					span.SetAttributes(attribute.String("request.id", id))
				}
				// Both spellings: current semantic conventions and the legacy one older
				// dashboards key on.
				span.SetAttributes(
					attribute.Int("http.response.status_code", status),
					attribute.Int("http.status_code", status),
					attribute.Int("http.response.body.size", rec.bytes),
				)
				if status >= 500 {
					span.SetStatus(codes.Error, strconv.Itoa(status))
				}
				// End before re-panicking: an End that runs during a panic records it again,
				// typed after the Go value ("string"), which is the grouping RecordPanic avoids.
				// The outer deferred End is then a no-op.
				span.End()
				if p != nil {
					panic(p)
				}
			}()
			next.ServeHTTP(rec, inner)
		})
	}
}

// recordServer counts one request. Same attribute names as galileo-django's request metrics;
// unmatched routes (404 scans) get no http.route rather than one series per probed path.
func (in *instruments) recordServer(ctx context.Context, method, route string, status int, who *Identity, d time.Duration) {
	in.serverActive.Add(ctx, -1, metric.WithAttributes(attribute.String("http.method", method)))
	attrs := make([]attribute.KeyValue, 0, 4)
	attrs = append(attrs, attribute.String("http.method", method), attribute.Int("http.status_code", status))
	if route != "" {
		attrs = append(attrs, attribute.String("http.route", route))
	}
	if who != nil && who.TenantID != "" {
		attrs = append(attrs, attribute.String("tenant.id", who.TenantID))
	}
	opt := metric.WithAttributes(attrs...)
	in.serverDuration.Record(ctx, ms(d), opt)
	in.serverRequests.Add(ctx, 1, opt)
}

// stdlibPattern is the pattern net/http's ServeMux matched ("GET /orders/{id}" → "/orders/{id}").
func stdlibPattern(r *http.Request) string {
	p := r.Pattern
	if i := strings.IndexByte(p, ' '); i >= 0 { // "GET /x" → "/x"
		p = p[i+1:]
	}
	if i := strings.IndexByte(p, '/'); i > 0 { // "host/x" → "/x"
		p = p[i:]
	}
	return p
}

type statusRecorder struct {
	http.ResponseWriter
	status int
	bytes  int
}

func (s *statusRecorder) WriteHeader(code int) {
	if s.status == 0 {
		s.status = code
	}
	s.ResponseWriter.WriteHeader(code)
}

func (s *statusRecorder) Write(b []byte) (int, error) {
	if s.status == 0 {
		s.status = http.StatusOK
	}
	n, err := s.ResponseWriter.Write(b)
	s.bytes += n
	return n, err
}

// Unwrap lets http.ResponseController reach Flush, Hijack, deadlines… on the real writer.
func (s *statusRecorder) Unwrap() http.ResponseWriter { return s.ResponseWriter }

// Flush keeps streaming (SSE) working through handlers that type-assert http.Flusher.
func (s *statusRecorder) Flush() {
	if f, ok := s.ResponseWriter.(http.Flusher); ok {
		f.Flush()
	}
}

// Transport wraps an http.RoundTripper (nil = http.DefaultTransport) so every outgoing request
// is a client span and carries traceparent: the called service's spans join this trace.
//
//	client := &http.Client{Transport: galileo.Transport(nil)}
//
// Always pass the request context (http.NewRequestWithContext), or the call starts a new trace.
// Each call is also timed in http.client.request.duration (method, server.address, status).
func Transport(base http.RoundTripper) http.RoundTripper {
	if base == nil {
		base = http.DefaultTransport
	}
	return roundTripper{base: base}
}

type roundTripper struct{ base http.RoundTripper }

func (rt roundTripper) RoundTrip(r *http.Request) (*http.Response, error) {
	ctx, span := Tracer().Start(r.Context(), r.Method+" "+r.URL.Host,
		trace.WithSpanKind(trace.SpanKindClient),
		trace.WithAttributes(
			attribute.String("http.request.method", r.Method),
			attribute.String("server.address", r.URL.Hostname()),
			attribute.String("url.full", redactURL(r)),
		))
	defer span.End()
	r = r.Clone(ctx)
	otel.GetTextMapPropagator().Inject(ctx, propagation.HeaderCarrier(r.Header))
	start := time.Now()
	resp, err := rt.base.RoundTrip(r)
	if in := active.Load(); in != nil {
		attrs := []attribute.KeyValue{attribute.String("http.method", r.Method), attribute.String("server.address", r.URL.Hostname())}
		if err != nil {
			attrs = append(attrs, attribute.String("error.type", errorType(err)))
		} else {
			attrs = append(attrs, attribute.Int("http.status_code", resp.StatusCode))
		}
		in.clientDuration.Record(ctx, ms(time.Since(start)), metric.WithAttributes(attrs...))
	}
	if err != nil {
		span.RecordError(err)
		span.SetStatus(codes.Error, err.Error())
		return resp, err
	}
	span.SetAttributes(attribute.Int("http.response.status_code", resp.StatusCode))
	if resp.StatusCode >= 500 {
		span.SetStatus(codes.Error, fmt.Sprintf("HTTP %d", resp.StatusCode))
	}
	return resp, nil
}

// redactURL drops the query string and user info: tokens and personal data often ride there.
func redactURL(r *http.Request) string {
	u := *r.URL
	u.User, u.RawQuery, u.Fragment = nil, "", ""
	return u.String()
}
