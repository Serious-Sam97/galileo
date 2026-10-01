// Package chiroute names request spans after chi's route template.
//
//	r := chi.NewRouter()
//	r.Use(tel.Middleware(galileo.WithRoute(chiroute.Pattern)))
package chiroute

import (
	"net/http"
	"strings"

	"github.com/go-chi/chi/v5"
)

// Pattern is the route chi matched, as registered ("/api/orders/{id}/"). chi's own
// RoutePattern() trims trailing slashes, which would merge "/x" and "/x/" routes, so the raw
// patterns of the sub-routers are joined instead.
func Pattern(r *http.Request) string {
	rctx := chi.RouteContext(r.Context())
	if rctx == nil || len(rctx.RoutePatterns) == 0 {
		return ""
	}
	p := strings.Join(rctx.RoutePatterns, "")
	// sub-router mounts contribute "/*" joints: "/api/*" + "/orders/{id}" → "/api/orders/{id}"
	for strings.Contains(p, "/*/") { // nested mounts can leave overlapping joints
		p = strings.ReplaceAll(p, "/*/", "/")
	}
	return p
}
