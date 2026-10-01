package pgxtrace_test

import (
	"context"
	"testing"

	"github.com/jackc/pgx/v5"
	"github.com/stretchr/testify/require"
	"go.opentelemetry.io/otel/sdk/trace/tracetest"

	galileo "github.com/Serious-Sam97/galileo/sdk/go"
	"github.com/Serious-Sam97/galileo/sdk/go/pgxtrace"
)

func TestOneChildPerStatementAndTheParentIsNeverClosed(t *testing.T) {
	rec := tracetest.NewSpanRecorder()
	tel, err := galileo.Init(context.Background(), galileo.Config{SpanProcessor: rec, CallSites: true})
	require.NoError(t, err)
	defer func() { _ = tel.Shutdown(context.Background()) }()

	var seen []string
	tr := &pgxtrace.Tracer{Observer: func(_ context.Context, sql string) { seen = append(seen, sql) }}
	ctx, parent := tel.Tracer().Start(context.Background(), "GET /x")

	qctx := tr.TraceQueryStart(ctx, nil, pgx.TraceQueryStartData{SQL: "select * from consultas where id = $1"})
	tr.TraceQueryEnd(qctx, nil, pgx.TraceQueryEndData{Err: pgx.ErrNoRows})
	require.Len(t, rec.Ended(), 1)
	child := rec.Ended()[0]
	require.Equal(t, "SELECT consultas", child.Name())
	require.Equal(t, parent.SpanContext().SpanID(), child.Parent().SpanID())
	require.Empty(t, child.Events(), "no rows is not an error")
	require.Equal(t, []string{"select * from consultas where id = $1"}, seen)

	// An End with no matching Start must not end the caller's span.
	tr.TraceQueryEnd(ctx, nil, pgx.TraceQueryEndData{})
	require.True(t, parent.IsRecording())
	parent.End()
	require.Len(t, rec.Ended(), 2)
}

func TestAFailedStatementIsAnError(t *testing.T) {
	rec := tracetest.NewSpanRecorder()
	tel, _ := galileo.Init(context.Background(), galileo.Config{SpanProcessor: rec})
	defer func() { _ = tel.Shutdown(context.Background()) }()
	tr := pgxtrace.New()
	ctx, parent := tel.Tracer().Start(context.Background(), "GET /x")
	defer parent.End()
	qctx := tr.TraceQueryStart(ctx, nil, pgx.TraceQueryStartData{SQL: "insert into pets values ($1)"})
	tr.TraceQueryEnd(qctx, nil, pgx.TraceQueryEndData{Err: context.DeadlineExceeded})
	child := rec.Ended()[0]
	require.Equal(t, "INSERT pets", child.Name())
	require.Len(t, child.Events(), 1)
}
