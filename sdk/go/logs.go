package galileo

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"strings"

	"go.opentelemetry.io/contrib/bridges/otelslog"
	"go.opentelemetry.io/otel/exporters/otlp/otlplog/otlploghttp"
	"go.opentelemetry.io/otel/log/global"
	sdklog "go.opentelemetry.io/otel/sdk/log"
)

func newLogExporter(ctx context.Context, cfg Config) (sdklog.Exporter, error) {
	opts := []otlploghttp.Option{otlploghttp.WithEndpointURL(strings.TrimRight(cfg.Endpoint, "/") + "/v1/logs")}
	if cfg.APIKey != "" {
		opts = append(opts, otlploghttp.WithHeaders(map[string]string{"Authorization": "Bearer " + cfg.APIKey}))
	}
	exporter, err := otlploghttp.New(ctx, opts...)
	if err != nil {
		return nil, fmt.Errorf("galileo: otlp log exporter: %w", err)
	}
	return exporter, nil
}

// SlogHandler sends every record at or above Config.LogLevel to Galileo and also hands it to
// local (your stdout handler; nil for none). Records logged with a context — slog.InfoContext,
// logger.ErrorContext — carry that request's trace id and identity, so Logs links to Traces and
// filters by user and tenant.
//
//	slog.SetDefault(slog.New(tel.SlogHandler(slog.NewJSONHandler(os.Stdout, nil))))
//
// Without log export (no endpoint, or GALILEO_LOGS=0) it returns local unchanged.
func (t *Telemetry) SlogHandler(local slog.Handler) slog.Handler {
	if t == nil || t.logs == nil {
		if local == nil {
			return slog.DiscardHandler
		}
		return local
	}
	remote := otelslog.NewHandler(ScopeName, otelslog.WithLoggerProvider(t.logs), otelslog.WithVersion(Version))
	return fanoutHandler{local: local, remote: remote, level: t.cfg.LogLevel}
}

// fanoutHandler gives each record to the local handler under its own level and to the OTLP
// handler from level up.
type fanoutHandler struct {
	local  slog.Handler
	remote slog.Handler
	level  slog.Leveler
}

func (h fanoutHandler) Enabled(ctx context.Context, l slog.Level) bool {
	return l >= h.level.Level() || (h.local != nil && h.local.Enabled(ctx, l))
}

func (h fanoutHandler) Handle(ctx context.Context, r slog.Record) error {
	var errs []error
	if h.local != nil && h.local.Enabled(ctx, r.Level) {
		errs = append(errs, h.local.Handle(ctx, r.Clone()))
	}
	if r.Level >= h.level.Level() {
		errs = append(errs, h.remote.Handle(ctx, r))
	}
	return errors.Join(errs...)
}

func (h fanoutHandler) WithAttrs(attrs []slog.Attr) slog.Handler {
	if h.local != nil {
		h.local = h.local.WithAttrs(attrs)
	}
	h.remote = h.remote.WithAttrs(attrs)
	return h
}

func (h fanoutHandler) WithGroup(name string) slog.Handler {
	if h.local != nil {
		h.local = h.local.WithGroup(name)
	}
	h.remote = h.remote.WithGroup(name)
	return h
}

// identityLogProcessor copies the context's identity onto every log record, top level whatever
// slog groups the record was logged under. Registered before the exporter so it sees them.
type identityLogProcessor struct{}

func (identityLogProcessor) OnEmit(ctx context.Context, r *sdklog.Record) error {
	id, ok := IdentityFrom(ctx)
	if !ok {
		return nil
	}
	r.AddAttributes(id.attributes()...)
	return nil
}
func (identityLogProcessor) Enabled(context.Context, sdklog.EnabledParameters) bool { return true }
func (identityLogProcessor) Shutdown(context.Context) error                         { return nil }
func (identityLogProcessor) ForceFlush(context.Context) error                       { return nil }

func newLoggerProvider(ctx context.Context, cfg Config) (*sdklog.LoggerProvider, error) {
	processor := cfg.LogProcessor
	if processor == nil {
		if cfg.Endpoint == "" || !cfg.Logs {
			return nil, nil
		}
		exporter, err := newLogExporter(ctx, cfg)
		if err != nil {
			return nil, err
		}
		processor = sdklog.NewBatchProcessor(exporter)
	}
	provider := sdklog.NewLoggerProvider(
		sdklog.WithResource(newResource(cfg)),
		sdklog.WithProcessor(identityLogProcessor{}),
		sdklog.WithProcessor(processor),
	)
	global.SetLoggerProvider(provider)
	return provider, nil
}
