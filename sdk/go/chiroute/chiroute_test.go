package chiroute_test

import (
	"context"
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/go-chi/chi/v5"
	"github.com/stretchr/testify/require"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"

	galileo "github.com/Serious-Sam97/galileo/sdk/go"
	"github.com/Serious-Sam97/galileo/sdk/go/chiroute"
)

func TestSpansAreNamedAfterChiRoutesIncludingMountsAndTrailingSlashes(t *testing.T) {
	rec := tracetest.NewSpanRecorder()
	tel, err := galileo.Init(context.Background(), galileo.Config{SpanProcessor: rec})
	require.NoError(t, err)
	defer func() { _ = tel.Shutdown(context.Background()) }()

	r := chi.NewRouter()
	r.Use(tel.Middleware(galileo.WithRoute(chiroute.Pattern)))
	r.Get("/api/consultas/{id}/", func(http.ResponseWriter, *http.Request) {})
	r.Route("/api/pets", func(sub chi.Router) {
		sub.Get("/{id}", func(http.ResponseWriter, *http.Request) {})
	})

	for _, path := range []string{"/api/consultas/12/", "/api/pets/3"} {
		r.ServeHTTP(httptest.NewRecorder(), httptest.NewRequest(http.MethodGet, path, nil))
	}
	spans := rec.Ended()
	require.Len(t, spans, 2)
	require.Equal(t, "GET /api/consultas/{id}/", spans[0].Name(), "a thousand visits are one row")
	require.Equal(t, "GET /api/pets/{id}", spans[1].Name())
}
