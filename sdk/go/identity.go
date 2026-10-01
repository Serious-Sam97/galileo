package galileo

import (
	"context"
	"net/http"

	"go.opentelemetry.io/otel"
	"go.opentelemetry.io/otel/attribute"
	"go.opentelemetry.io/otel/propagation"
	sdktrace "go.opentelemetry.io/otel/sdk/trace"
	"go.opentelemetry.io/otel/trace"
)

// Identity is who is acting and for which tenant. Ids travel as strings: Galileo groups and
// filters on them as opaque text.
type Identity struct {
	UserID     string
	Email      string
	Name       string
	TenantID   string
	TenantName string
	// Attributes are extra identity attributes (e.g. "tenant.domain").
	Attributes []attribute.KeyValue
}

type identityKey struct{}

// identitySlotKey holds a *Identity the HTTP middleware reads after the handler ran: identity is
// resolved on an inner context, but the request metric is recorded by the outer middleware.
type identitySlotKey struct{}

// WithIdentity stores id in the context and puts it on the span already open there (usually the
// request span, opened before authentication ran). Every span started from the returned context
// — SQL, cache, your own — carries it too.
func WithIdentity(ctx context.Context, id Identity) context.Context {
	if span := trace.SpanFromContext(ctx); span.IsRecording() {
		span.SetAttributes(id.attributes()...)
	}
	if slot, ok := ctx.Value(identitySlotKey{}).(*Identity); ok {
		*slot = id
	}
	return context.WithValue(ctx, identityKey{}, id)
}

// IdentityFrom returns the identity stored by [WithIdentity].
func IdentityFrom(ctx context.Context) (Identity, bool) {
	id, ok := ctx.Value(identityKey{}).(Identity)
	return id, ok
}

func (id Identity) attributes() []attribute.KeyValue {
	out := make([]attribute.KeyValue, 0, 5+len(id.Attributes))
	add := func(k, v string) {
		if v != "" {
			out = append(out, attribute.String(k, v))
		}
	}
	add("user.id", id.UserID)
	add("user.email", id.Email)
	add("user.name", id.Name)
	add("tenant.id", id.TenantID)
	add("tenant.name", id.TenantName)
	return append(out, id.Attributes...)
}

// Identify is middleware that runs after your authentication and tenant resolution: resolve
// reads them from the request (return ok=false for anonymous requests).
//
//	r.Use(galileo.Identify(func(r *http.Request) (galileo.Identity, bool) {
//		u := auth.UserFrom(r.Context())
//		if u == nil { return galileo.Identity{}, false }
//		return galileo.Identity{UserID: strconv.FormatInt(u.ID, 10), TenantID: u.ClinicID}, true
//	}))
func Identify(resolve func(*http.Request) (Identity, bool)) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			if id, ok := resolve(r); ok {
				r = r.WithContext(WithIdentity(r.Context(), id))
			}
			next.ServeHTTP(w, r)
		})
	}
}

// identityProcessor copies the context's identity onto every span as it starts.
type identityProcessor struct{}

func (identityProcessor) OnStart(parent context.Context, s sdktrace.ReadWriteSpan) {
	if id, ok := IdentityFrom(parent); ok {
		s.SetAttributes(id.attributes()...)
	}
}
func (identityProcessor) OnEnd(sdktrace.ReadOnlySpan)      {}
func (identityProcessor) Shutdown(context.Context) error   { return nil }
func (identityProcessor) ForceFlush(context.Context) error { return nil }

// GatewayHeaders are the headers for a call to the Galileo LLM gateway made while handling ctx:
// traceparent (the LLM span joins the request trace) and x-galileo-user-id / x-galileo-tenant-id
// (per-user and per-tenant cost, budgets and guardrails).
//
//	req.Header = galileo.GatewayHeaders(ctx, req.Header)
func GatewayHeaders(ctx context.Context, h http.Header) http.Header {
	if h == nil {
		h = http.Header{}
	}
	otel.GetTextMapPropagator().Inject(ctx, propagation.HeaderCarrier(h))
	if id, ok := IdentityFrom(ctx); ok {
		if id.UserID != "" {
			h.Set("x-galileo-user-id", id.UserID)
		}
		if id.TenantID != "" {
			h.Set("x-galileo-tenant-id", id.TenantID)
		}
	}
	return h
}
