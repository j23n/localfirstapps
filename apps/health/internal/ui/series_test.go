package ui

import (
	"strings"
	"testing"
)

func TestDisplayAmountHealthKitPercent(t *testing.T) {
	// Fixture WalkingDoubleSupportPercentage value="0.312" unit="%"
	v, u := displayAmount(0.312, "%")
	if v != 31.2 || u != "%" {
		t.Fatalf("double support: %v %s", v, u)
	}
	s, u := formatAmount(0.28, "%")
	if s != "28" || u != "%" {
		t.Fatalf("0.28 %% → %q %s want 28 %%", s, u)
	}
	s, u = formatAmount(0.97, "%")
	if s != "97" || u != "%" {
		t.Fatalf("SpO2 0.97 → %q %s", s, u)
	}
	s, u = formatAmount(0, "%")
	if s != "0" || u != "%" {
		t.Fatalf("zero: %q %s", s, u)
	}
	// Already percent points (would be > 1 as a fraction).
	s, u = formatAmount(28, "%")
	if s != "28" || u != "%" {
		t.Fatalf("already points: %q %s", s, u)
	}
	s, u = formatAmount(0.312, "count")
	if s != "0.31" || u != "count" {
		t.Fatalf("non-percent must not scale: %q %s", s, u)
	}
}

func TestSeriesJSONScalesPercent(t *testing.T) {
	v := 0.312
	js := string(seriesJSON([]plotPoint{
		{Label: "8 Aug", Value: &v, Unit: "%", FromZero: true},
	}, "Double support", "Day"))
	if !strings.Contains(js, "31.2") {
		t.Fatalf("chart data should be percent points: %s", js)
	}
	if strings.Contains(js, "0.312") {
		t.Fatalf("raw fraction leaked into chart: %s", js)
	}
	if !strings.Contains(js, `"xTitle":"Day"`) || !strings.Contains(js, `"yTitle":"%"`) {
		t.Fatalf("axis titles: %s", js)
	}
}
