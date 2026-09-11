package ui

import (
	"math"
	"strings"
	"testing"
	"time"

	"archive/internal/projection"
)

func TestEachBucketCounts(t *testing.T) {
	loc := time.UTC
	from := time.Date(2026, 8, 18, 0, 0, 0, 0, loc)
	to := time.Date(2026, 8, 24, 0, 0, 0, 0, loc) // 7 days
	if n := len(eachBucket(from, to, grainHour, loc)); n != 7*24 {
		t.Fatalf("7d hours=%d want 168", n)
	}
	if n := len(eachBucket(to, to, grainHour, loc)); n != 24 {
		t.Fatalf("1d hours=%d want 24", n)
	}
	aug1 := time.Date(2026, 8, 1, 0, 0, 0, 0, loc)
	aug31 := time.Date(2026, 8, 31, 0, 0, 0, 0, loc)
	if n := len(eachBucket(aug1, aug31, grainDay, loc)); n != 31 {
		t.Fatalf("August days=%d want 31", n)
	}
	jan := time.Date(2026, 1, 1, 0, 0, 0, 0, loc)
	dec := time.Date(2026, 12, 1, 0, 0, 0, 0, loc)
	if n := len(eachBucket(jan, dec, grainMonth, loc)); n != 12 {
		t.Fatalf("year months=%d want 12", n)
	}
}

func TestObservationPlotDayKeepsSampleRange(t *testing.T) {
	loc := time.UTC
	rows := []projection.ObservationRow{
		{Start: "2026-08-24T08:00:00Z", Value: "12", Unit: "count/min"},
		{Start: "2026-08-24T20:00:00Z", Value: "18", Unit: "count/min"},
	}
	pts := observationPlot(rows, "2026-08-24", "2026-08-24", "month", loc, "mean")
	if len(pts) != 1 || pts[0].Value == nil || *pts[0].Value != 15 {
		t.Fatalf("%+v", pts)
	}
	if pts[0].Lo == nil || pts[0].Hi == nil || *pts[0].Lo != 12 || *pts[0].Hi != 18 {
		t.Fatalf("month bar should be the day's min–max, got %+v", pts[0])
	}
}

func TestObservationPlotKeepsEmptyDays(t *testing.T) {
	loc := time.UTC
	rows := []projection.ObservationRow{
		{Start: "2026-08-24T12:00:00Z", Value: "16", Unit: "count/min", Source: "Watch"},
	}
	pts := observationPlot(rows, "2026-08-01", "2026-08-24", "month", loc, "mean")
	if len(pts) != 24 {
		t.Fatalf("len=%d want 24 (1–24 Aug)", len(pts))
	}
	filled := 0
	for _, p := range pts {
		if p.Value != nil {
			filled++
			if *p.Value != 16 {
				t.Fatalf("value=%v", *p.Value)
			}
		}
	}
	if filled != 1 {
		t.Fatalf("filled=%d want 1 (empty days stay slots)", filled)
	}
	if pts[len(pts)-1].Value == nil {
		t.Fatal("last day should hold the sample")
	}
	if pts[0].Value != nil {
		t.Fatal("1 Aug should be empty")
	}
}

func TestObservationPlotHourlySevenDays(t *testing.T) {
	loc := time.UTC
	rows := []projection.ObservationRow{
		{Start: "2026-08-24T15:10:00Z", Value: "12", Unit: "count/min"},
		{Start: "2026-08-24T15:40:00Z", Value: "18", Unit: "count/min"},
	}
	pts := observationPlot(rows, "2026-08-18", "2026-08-24", "7d", loc, "mean")
	if len(pts) != 168 {
		t.Fatalf("len=%d want 168", len(pts))
	}
	var hit *plotPoint
	for i := range pts {
		if pts[i].Value != nil {
			if hit != nil {
				t.Fatalf("extra hour filled: %s", pts[i].Label)
			}
			hit = &pts[i]
		}
	}
	if hit == nil || *hit.Value != 15 {
		t.Fatalf("hour mean=%v", hit)
	}
	if hit.Lo == nil || hit.Hi == nil || *hit.Lo != 12 || *hit.Hi != 18 {
		t.Fatalf("range %+v", hit)
	}
	if hit.FromZero {
		t.Fatal("mean kind should use range bars, not bars-from-zero")
	}
}

func TestObservationPlotHourlyOneDay(t *testing.T) {
	loc := time.UTC
	rows := []projection.ObservationRow{
		{Start: "2026-08-24T15:10:00Z", Value: "12", Unit: "count/min"},
	}
	pts := observationPlot(rows, "2026-08-24", "2026-08-24", "1d", loc, "mean")
	if len(pts) != 24 {
		t.Fatalf("len=%d want 24", len(pts))
	}
	var n int
	for _, p := range pts {
		if p.Value != nil {
			n++
		}
	}
	if n != 1 {
		t.Fatalf("filled hours=%d", n)
	}
}

func TestObservationPlotMonthlyYear(t *testing.T) {
	loc := time.UTC
	rows := []projection.ObservationRow{
		{Start: "2025-12-02T00:00:00Z", Value: "10", Unit: "count/min"},
		{Start: "2026-08-24T00:00:00Z", Value: "20", Unit: "count/min"},
	}
	pts := observationPlot(rows, "2026-01-01", "2026-12-31", "year", loc, "mean")
	if len(pts) != 12 {
		t.Fatalf("months=%d want 12", len(pts))
	}
	var n int
	for _, p := range pts {
		if p.Value != nil {
			n++
		}
	}
	if n != 1 {
		t.Fatalf("filled months=%d want 1 (Aug 2026)", n)
	}
}

func TestSeriesJSONRangeAndNulls(t *testing.T) {
	v, lo, hi := 15.0, 12.0, 18.0
	js := string(seriesJSON([]plotPoint{
		{Label: "empty"},
		{Label: "hit", Value: &v, Lo: &lo, Hi: &hi, Unit: "bpm"},
	}, "Heart rate", "Hour"))
	if !strings.Contains(js, "null") || !strings.Contains(js, "[12,18]") {
		t.Fatalf("want null slot + range bar: %s", js)
	}
	if !strings.Contains(js, `"rangeBars":true`) || strings.Contains(js, `"beginAtZero":true`) {
		t.Fatalf("range style: %s", js)
	}
}

func TestCalendarValuesLeaveGaps(t *testing.T) {
	loc := time.UTC
	vals := calendarValues([]dayVal{
		{Day: "2026-08-24", Value: 10},
	}, "2026-08-24", 3, loc)
	if len(vals) != 3 {
		t.Fatalf("%v", vals)
	}
	if !math.IsNaN(vals[0]) || !math.IsNaN(vals[1]) || vals[2] != 10 {
		t.Fatalf("gaps: %v", vals)
	}
}

func TestShiftDayAndWindowLabel(t *testing.T) {
	loc := time.UTC
	if g := shiftDay("2026-08-24", "1d", -1, loc); g != "2026-08-23" {
		t.Fatalf("1d prev %s", g)
	}
	if g := shiftDay("2026-08-24", "7d", -1, loc); g != "2026-08-17" {
		t.Fatalf("7d prev %s", g)
	}
	from, to := rangeDates("2026-08-24", "1d", loc)
	if from != "2026-08-24" || to != "2026-08-24" {
		t.Fatalf("1d window %s %s", from, to)
	}
	if g := windowCaption("1d", "2026-08-24", "2026-08-24", "2026-08-24", loc); g != "24 August 2026" {
		t.Fatalf("1d caption %q", g)
	}
	if g := shiftDay("2026-08-24", "month", -1, loc); g != "2026-07-31" {
		t.Fatalf("month prev %s", g)
	}
	if g := shiftDay("2026-08-24", "year", -1, loc); g != "2025-12-31" {
		t.Fatalf("year prev %s", g)
	}
	if g := windowCaption("month", "2026-08-01", "2026-08-24", "2026-08-24", loc); g != "August 2026" {
		t.Fatalf("month caption %q", g)
	}
	if g := windowCaption("year", "2026-01-01", "2026-08-24", "2026-08-24", loc); g != "2026" {
		t.Fatalf("year caption %q", g)
	}
	from, to = rangeDates("2026-08-24", "month", loc)
	if from != "2026-08-01" || to != "2026-08-31" {
		t.Fatalf("month window %s %s", from, to)
	}
	from, to = rangeDates("2026-08-24", "year", loc)
	if from != "2026-01-01" || to != "2026-12-31" {
		t.Fatalf("year window %s %s", from, to)
	}
	if normalizeRange("30d") != "month" || normalizeRange("1y") != "year" || normalizeRange("90d") != "month" {
		t.Fatal("old range tokens should alias to month/year")
	}
}

func TestEpisodePlotUsesDaysOnSevenDayRange(t *testing.T) {
	loc := time.UTC
	pts := episodePlot(map[string]int{"2026-08-24": 2}, "2026-08-18", "2026-08-24", "7d", loc)
	if len(pts) != 7 {
		t.Fatalf("workout 7d should be daily, got %d", len(pts))
	}
}
