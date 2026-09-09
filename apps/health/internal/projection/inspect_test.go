package projection

import (
	"strings"
	"testing"
	"time"

	_ "time/tzdata"
)

func TestLocalWindowOffset(t *testing.T) {
	loc, err := ParseLocation("+0200")
	if err != nil {
		t.Fatal(err)
	}
	w, err := LocalWindow("2025-09-12", "", "", loc)
	if err != nil {
		t.Fatal(err)
	}
	if w.Offset != "+0200" {
		t.Fatalf("offset=%s", w.Offset)
	}
	if w.FromUTC != time.Date(2025, 9, 11, 22, 0, 0, 0, time.UTC) {
		t.Fatalf("from=%s", w.FromUTC)
	}
	if w.ToUTC != time.Date(2025, 9, 12, 22, 0, 0, 0, time.UTC) {
		t.Fatalf("to=%s", w.ToUTC)
	}
	note := w.Note()
	if !strings.Contains(note, "offset +0200") || !strings.Contains(note, "2025-09-11T22:00:00Z") {
		t.Fatalf("note=%s", note)
	}
}

func TestDSTSpringForward2026(t *testing.T) {
	loc, err := time.LoadLocation("Europe/Berlin")
	if err != nil {
		t.Fatal(err)
	}
	w, err := LocalWindow("2026-03-29", "", "", loc)
	if err != nil {
		t.Fatal(err)
	}
	if w.Offset != "+0100" {
		t.Fatalf("offset=%s want +0100 (CET before the jump)", w.Offset)
	}
	wantFrom := time.Date(2026, 3, 28, 23, 0, 0, 0, time.UTC)
	wantTo := time.Date(2026, 3, 29, 22, 0, 0, 0, time.UTC)
	if !w.FromUTC.Equal(wantFrom) || !w.ToUTC.Equal(wantTo) {
		t.Fatalf("window %s .. %s", w.FromUTC, w.ToUTC)
	}
	if d := w.ToUTC.Sub(w.FromUTC); d != 23*time.Hour {
		t.Fatalf("spring-forward day is %s, want 23h", d)
	}
}

func TestDSTFallBack2026(t *testing.T) {
	loc, err := time.LoadLocation("Europe/Berlin")
	if err != nil {
		t.Fatal(err)
	}
	w, err := LocalWindow("2026-10-25", "", "", loc)
	if err != nil {
		t.Fatal(err)
	}
	if w.Offset != "+0200" {
		t.Fatalf("offset=%s want +0200 (CEST before the fallback)", w.Offset)
	}
	wantFrom := time.Date(2026, 10, 24, 22, 0, 0, 0, time.UTC)
	wantTo := time.Date(2026, 10, 25, 23, 0, 0, 0, time.UTC)
	if !w.FromUTC.Equal(wantFrom) || !w.ToUTC.Equal(wantTo) {
		t.Fatalf("window %s .. %s", w.FromUTC, w.ToUTC)
	}
	if d := w.ToUTC.Sub(w.FromUTC); d != 25*time.Hour {
		t.Fatalf("fall-back day is %s, want 25h", d)
	}
}

func TestDayWindowInclusiveStartExclusiveEnd(t *testing.T) {
	loc, err := ParseLocation("+0200")
	if err != nil {
		t.Fatal(err)
	}
	w, err := LocalWindow("2026-06-06", "", "", loc)
	if err != nil {
		t.Fatal(err)
	}
	// 00:01 local on the 6th is 2026-06-05T22:01:00Z — inside.
	in := time.Date(2026, 6, 5, 22, 1, 37, 0, time.UTC)
	// exactly next local midnight is 2026-06-06T22:00:00Z — outside.
	end := time.Date(2026, 6, 6, 22, 0, 0, 0, time.UTC)
	if in.Before(w.FromUTC) || !in.Before(w.ToUTC) {
		t.Fatalf("00:01 local should be inside [%s, %s)", w.FromUTC, w.ToUTC)
	}
	if !end.Equal(w.ToUTC) {
		t.Fatalf("upper bound %s want %s", w.ToUTC, end)
	}
	if !end.Before(w.ToUTC) && !end.Equal(w.ToUTC) {
		t.Fatal("end should equal ToUTC")
	}
	// start_ts < ToUTC, so a row timestamped exactly ToUTC is excluded.
	if !w.ToUTC.Equal(end) || end.Before(w.ToUTC) {
		t.Fatal("exact next-midnight must not satisfy start_ts < ToUTC")
	}
}

func TestResolveKind(t *testing.T) {
	names := []string{
		"HKQuantityTypeIdentifierStepCount",
		"HKQuantityTypeIdentifierHeartRate",
		"HKQuantityTypeIdentifierRestingHeartRate",
		"HKCategoryTypeIdentifierSleepAnalysis",
		"HKWorkoutActivityTypeRunning",
	}
	got, err := ResolveKind(names, "StepCount")
	if err != nil || got != "HKQuantityTypeIdentifierStepCount" {
		t.Fatalf("StepCount: %s %v", got, err)
	}
	got, err = ResolveKind(names, "HKQuantityTypeIdentifierHeartRate")
	if err != nil || got != "HKQuantityTypeIdentifierHeartRate" {
		t.Fatalf("exact: %s %v", got, err)
	}
	if _, err := ResolveKind(names, "HeartRate"); err == nil || !strings.Contains(err.Error(), "ambiguous") {
		t.Fatalf("expected ambiguous HeartRate, got %v", err)
	}
	if _, err := ResolveKind(names, "NoSuch"); err == nil {
		t.Fatal("expected unknown")
	}
	got, err = ResolveKind(names, "SleepAnalysis")
	if err != nil || got != "HKCategoryTypeIdentifierSleepAnalysis" {
		t.Fatalf("SleepAnalysis: %s %v", got, err)
	}
}

func TestSumRefusesMultiSource(t *testing.T) {
	rows := []ObservationRow{
		{Kind: "HKQuantityTypeIdentifierStepCount", Source: "Apple Watch", Value: "10", Unit: "count"},
		{Kind: "HKQuantityTypeIdentifierStepCount", Source: "iPhone", Value: "5", Unit: "count"},
	}
	if _, err := SumObservations(rows, false, "+0200"); err == nil || !strings.Contains(err.Error(), "refusing to sum across sources") {
		t.Fatalf("expected refuse, got %v", err)
	}
	sums, err := SumObservations(rows, true, "+0200")
	if err != nil {
		t.Fatal(err)
	}
	if len(sums) != 2 {
		t.Fatalf("by-source groups=%d", len(sums))
	}
}

func TestListObservationsWindowBounds(t *testing.T) {
	root := t.TempDir()
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	db, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	loc, err := ParseLocation("+0200")
	if err != nil {
		t.Fatal(err)
	}
	w, err := LocalWindow("2026-06-06", "", "", loc)
	if err != nil {
		t.Fatal(err)
	}
	start := w.FromUTC.Format("2006-01-02T15:04:05.000000000Z")
	end := w.ToUTC.Format("2006-01-02T15:04:05.000000000Z")
	before := w.FromUTC.Add(-time.Second).Format("2006-01-02T15:04:05.000000000Z")
	ins := `INSERT INTO observations (dedup_key, n, kind, source, start_ts, end_ts, metadata, value) VALUES (?, 1, 'HKQuantityTypeIdentifierRestingHeartRate', 'Apple Watch', ?, ?, '{}', '45')`
	for i, ts := range []string{before, start, end} {
		if _, err := db.Exec(ins, string(rune('a'+i)), ts, ts); err != nil {
			t.Fatal(err)
		}
	}
	rows, err := ListObservations(db, ObsQuery{Kind: "HKQuantityTypeIdentifierRestingHeartRate", Window: w})
	if err != nil {
		t.Fatal(err)
	}
	if len(rows) != 1 || rows[0].Start != start {
		t.Fatalf("want only the inclusive start row, got %+v", rows)
	}
}
