package ui

import (
	"strconv"
	"testing"
	"time"

	"archive/internal/projection"
)

func TestPercentileKnownDistribution(t *testing.T) {
	hist := make([]float64, 30)
	for i := range hist {
		hist[i] = float64(i + 1) // 1..30
	}
	if p := Percentile(30, hist); p < 90 {
		t.Fatalf("30 vs 1..30 percentile=%v want ≥90", p)
	}
	if p := Percentile(1, hist); p >= 10 {
		t.Fatalf("1 vs 1..30 percentile=%v want <10", p)
	}
	if p := Percentile(15, hist); p < 10 || p > 90 {
		t.Fatalf("15 vs 1..30 percentile=%v want mid", p)
	}
	if n := BaselineNote(30, hist); n != "Higher than usual for you" {
		t.Fatalf("note=%q", n)
	}
	if n := BaselineNote(1, hist); n != "Lower than usual for you" {
		t.Fatalf("note=%q", n)
	}
	if n := BaselineNote(15, hist); n != "" {
		t.Fatalf("mid should be undecorated, got %q", n)
	}
}

func TestBaselineSuppressedBelow30Days(t *testing.T) {
	hist := make([]float64, 29)
	for i := range hist {
		hist[i] = float64(i)
	}
	if n := BaselineNote(1000, hist); n != "" {
		t.Fatalf("29 days must not annotate: %q", n)
	}
}

func TestBaselineUsesSingleSource(t *testing.T) {
	loc := time.FixedZone("+0200", 2*3600)
	var rows []projection.ObservationRow
	// 30 Watch days at 100, 30 iPhone days at 9000 — summing would flip the percentile.
	start, _ := time.ParseInLocation("2006-01-02", "2026-01-01", loc)
	for i := 0; i < 30; i++ {
		d := start.AddDate(0, 0, i)
		ts := d.UTC().Format(time.RFC3339Nano)
		rows = append(rows,
			projection.ObservationRow{Kind: "HKQuantityTypeIdentifierStepCount", Source: "Apple Watch", Start: ts, Value: strconv.Itoa(90 + i), Unit: "count"},
			projection.ObservationRow{Kind: "HKQuantityTypeIdentifierStepCount", Source: "iPhone", Start: ts, Value: "9000", Unit: "count"},
		)
	}
	var watch, phone []projection.ObservationRow
	for _, r := range rows {
		if r.Source == "Apple Watch" {
			watch = append(watch, r)
		} else {
			phone = append(phone, r)
		}
	}
	w := dailySeries(watch, loc, "sum")
	p := dailySeries(phone, loc, "sum")
	if len(w) != 30 || len(p) != 30 {
		t.Fatalf("days watch=%d phone=%d", len(w), len(p))
	}
	var wh, ph []float64
	for _, d := range w {
		wh = append(wh, d.Value)
	}
	for _, d := range p {
		ph = append(ph, d.Value)
	}
	// Today Watch=104 sits mid-pack for Watch (90..119), but is far below iPhone's 9000s.
	if n := BaselineNote(104, wh); n != "" {
		t.Fatalf("watch-only 104 should be usual: %q", n)
	}
	if n := BaselineNote(104, ph); n != "Lower than usual for you" {
		t.Fatalf("against phone-only 9000s, 104 is lower: %q", n)
	}
	if BaselineNote(104, wh) == BaselineNote(104, ph) {
		t.Fatal("Watch-only and iPhone-only baselines must not agree")
	}
}

func TestDailySeriesDoesNotSumSources(t *testing.T) {
	loc := time.UTC
	rows := []projection.ObservationRow{
		{Start: "2026-06-06T07:25:00Z", Value: "826", Source: "Apple Watch", Unit: "count"},
		{Start: "2026-06-06T07:25:00Z", Value: "837", Source: "iPhone", Unit: "count"},
	}
	w := dailySeries([]projection.ObservationRow{rows[0]}, loc, "sum")
	if len(w) != 1 || w[0].Value != 826 {
		t.Fatalf("%+v", w)
	}
}

func TestPreferRowsPreferredWinsOnCollision(t *testing.T) {
	loc := time.FixedZone("+0200", 2*3600)
	rows := []projection.ObservationRow{
		{Start: "2026-06-06T07:25:00Z", Value: "826", Source: "Apple Watch", Unit: "count"},
		{Start: "2026-06-06T08:00:00Z", Value: "837", Source: "iPhone", Unit: "count"},
	}
	got := preferRows(rows, "", "Apple Watch", loc)
	if len(got) != 1 || got[0].Source != "Apple Watch" || got[0].Value != "826" {
		t.Fatalf("%+v", got)
	}
	days := dailySeries(got, loc, "sum")
	if len(days) != 1 || days[0].Value != 826 || days[0].Source != "Apple Watch" {
		t.Fatalf("summed or dropped: %+v", days)
	}
}

func TestPreferRowsKeepsWhoeverRecorded(t *testing.T) {
	loc := time.FixedZone("+0200", 2*3600)
	rows := []projection.ObservationRow{
		{Start: "2026-06-06T07:25:00Z", Value: "826", Source: "Apple Watch", Unit: "count"},
		{Start: "2026-08-24T05:52:47Z", Value: "232", Source: "iPhone", Unit: "count"},
	}
	got := preferRows(rows, "", "Apple Watch", loc)
	if len(got) != 2 {
		t.Fatalf("want both days, got %+v", got)
	}
	days := dailySeries(got, loc, "sum")
	if len(days) != 2 || days[0].Value != 826 || days[1].Value != 232 {
		t.Fatalf("%+v", days)
	}
	if days[0].Source != "Apple Watch" || days[1].Source != "iPhone" {
		t.Fatalf("sources %+v", days)
	}
}

func TestPreferRowsLock(t *testing.T) {
	loc := time.UTC
	rows := []projection.ObservationRow{
		{Start: "2026-06-06T07:25:00Z", Value: "826", Source: "Apple Watch", Unit: "count"},
		{Start: "2026-08-24T05:52:47Z", Value: "232", Source: "iPhone", Unit: "count"},
	}
	got := preferRows(rows, "iPhone", "Apple Watch", loc)
	if len(got) != 1 || got[0].Source != "iPhone" {
		t.Fatalf("%+v", got)
	}
}
