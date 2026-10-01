package galileo

import (
	"testing"

	"github.com/stretchr/testify/require"
	sdkmetric "go.opentelemetry.io/otel/sdk/metric"
	"go.opentelemetry.io/otel/sdk/metric/metricdata"
)

func TestCountersAndHistogramsAreDeltasAndLevelsStayAbsolute(t *testing.T) {
	for kind, want := range map[sdkmetric.InstrumentKind]metricdata.Temporality{
		sdkmetric.InstrumentKindCounter:                 metricdata.DeltaTemporality,
		sdkmetric.InstrumentKindHistogram:               metricdata.DeltaTemporality,
		sdkmetric.InstrumentKindObservableCounter:       metricdata.DeltaTemporality,
		sdkmetric.InstrumentKindUpDownCounter:           metricdata.CumulativeTemporality,
		sdkmetric.InstrumentKindObservableUpDownCounter: metricdata.CumulativeTemporality,
		sdkmetric.InstrumentKindObservableGauge:         metricdata.CumulativeTemporality,
	} {
		require.Equal(t, want, deltaTemporality(kind), kind.String())
	}
}
