package main

import (
	"bytes"
	"strings"
	"testing"
)

func runCmd(t *testing.T, args ...string) (int, string, string) {
	t.Helper()
	var out, errb bytes.Buffer
	code := run(&out, &errb, args)
	return code, out.String(), errb.String()
}

func appleRoot(t *testing.T) string {
	t.Helper()
	t.Setenv("ARCHIVE_TZ", "+0200")
	root := t.TempDir()
	z := zipFixture(t)
	runOK(t, "-root", root, "import", "-dev", "apple", z)
	runOK(t, "-root", root, "rebuild")
	return root
}

func TestOnDayBoundaries(t *testing.T) {
	root := appleRoot(t)

	// 2025-09-12 00:27:33 +0200 → 2025-09-11T22:27:33Z (local Sep 12, UTC Sep 11)
	code, out, errb := runCmd(t, "-root", root, "observations", "-kind", "StepCount", "-on", "2025-09-12")
	if code != 0 {
		t.Fatalf("code=%d err=%s", code, errb)
	}
	if !strings.Contains(errb, "offset +0200") {
		t.Fatalf("missing offset note: %s", errb)
	}
	if !strings.Contains(errb, "2025-09-11T22:00:00Z") || !strings.Contains(errb, "2025-09-12T22:00:00Z") {
		t.Fatalf("UTC window: %s", errb)
	}
	if !strings.Contains(out, "2025-09-11T22:27:33") {
		t.Fatalf("local Sep 12 should include 00:27+0200 sample:\n%s", out)
	}
	if strings.Contains(out, "2025-09-11T21:49:08") {
		t.Fatalf("local Sep 12 should not include 23:49+0200 previous day:\n%s", out)
	}

	code, out, errb = runCmd(t, "-root", root, "observations", "-kind", "StepCount", "-on", "2025-09-11")
	if code != 0 {
		t.Fatalf("code=%d err=%s", code, errb)
	}
	if !strings.Contains(out, "2025-09-11T21:49:08") {
		t.Fatalf("local Sep 11 should include 23:49+0200:\n%s", out)
	}
	if strings.Contains(out, "2025-09-11T22:27:33") {
		t.Fatalf("local Sep 11 should not include 00:27+0200 next day:\n%s", out)
	}
}

func TestSumRefusesMultiSource(t *testing.T) {
	root := appleRoot(t)
	code, _, errb := runCmd(t, "-root", root, "observations", "-kind", "StepCount", "-sum")
	if code == 0 {
		t.Fatal("sum across multiple sources should refuse")
	}
	if !strings.Contains(errb, "refusing to sum across sources") {
		t.Fatalf("err=%s", errb)
	}
	if !strings.Contains(errb, "Apple Watch") || !strings.Contains(errb, ",") {
		t.Fatalf("should name the sources: %s", errb)
	}

	code, out, errb := runCmd(t, "-root", root, "observations", "-kind", "StepCount", "-sum", "-by-source")
	if code != 0 {
		t.Fatalf("by-source: %s", errb)
	}
	if !strings.Contains(out, "Apple Watch") || strings.Count(out, "\n") < 2 {
		t.Fatalf("by-source output: %s", out)
	}

	code, out, errb = runCmd(t, "-root", root, "observations", "-kind", "StepCount", "-source", "Apple Watch", "-sum")
	if code != 0 {
		t.Fatalf("single source: %s", errb)
	}
	if !strings.Contains(out, "Apple Watch") {
		t.Fatalf("sum: %s", out)
	}
}

func TestKindSuffixMatch(t *testing.T) {
	root := appleRoot(t)
	code, out, errb := runCmd(t, "-root", root, "observations", "-kind", "StepCount", "-json")
	if code != 0 {
		t.Fatalf("%s", errb)
	}
	if !strings.Contains(out, `"kind":"HKQuantityTypeIdentifierStepCount"`) {
		t.Fatalf("suffix StepCount: %s", out[:min(300, len(out))])
	}

	code, _, errb = runCmd(t, "-root", root, "observations", "-kind", "HeartRate")
	if code == 0 || !strings.Contains(errb, "ambiguous") {
		t.Fatalf("HeartRate should be ambiguous, got code=%d err=%s", code, errb)
	}
	if !strings.Contains(errb, "HKQuantityTypeIdentifierHeartRate") || !strings.Contains(errb, "RestingHeartRate") {
		t.Fatalf("ambiguity should list matches: %s", errb)
	}

	code, out, errb = runCmd(t, "-root", root, "observations", "-kind", "HKQuantityTypeIdentifierHeartRate", "-json")
	if code != 0 {
		t.Fatalf("exact kind: %s", errb)
	}
	if strings.Contains(out, "RestingHeartRate") {
		t.Fatal("exact HeartRate pulled RestingHeartRate")
	}

	code, _, errb = runCmd(t, "-root", root, "observations", "-kind", "NotAType")
	if code == 0 || !strings.Contains(errb, "unknown kind") {
		t.Fatalf("unknown: code=%d err=%s", code, errb)
	}
}

func TestBySourceWithoutSum(t *testing.T) {
	root := appleRoot(t)
	// -by-source alone must aggregate, not dump raw rows.
	code, out, errb := runCmd(t, "-root", root, "observations", "-kind", "StepCount", "-by-source")
	if code != 0 {
		t.Fatalf("%s", errb)
	}
	if strings.Contains(out, "HKQuantityTypeIdentifier") || strings.Count(out, "\n") > 8 {
		t.Fatalf("expected one line per source, got raw dump:\n%s", out)
	}
	if !strings.Contains(out, "Apple Watch") || strings.Count(strings.TrimSpace(out), "\n") < 1 {
		t.Fatalf("missing sources:\n%s", out)
	}
	if !strings.Contains(out, "sum=") || !strings.Contains(out, "n=") {
		t.Fatalf("missing SUM/COUNT:\n%s", out)
	}
}

func TestSleepAnalysisSuffix(t *testing.T) {
	root := appleRoot(t)
	code, out, errb := runCmd(t, "-root", root, "observations", "-kind", "SleepAnalysis")
	if code != 0 {
		t.Fatalf("observations SleepAnalysis: %s", errb)
	}
	if !strings.Contains(out, "SleepAnalysis") && !strings.Contains(out, "HKCategoryValueSleepAnalysis") {
		t.Fatalf("expected sleep rows:\n%s", out)
	}
	code, out, errb = runCmd(t, "-root", root, "episodes", "-kind", "SleepAnalysis")
	if code != 0 {
		t.Fatalf("episodes SleepAnalysis: %s", errb)
	}
	if !strings.Contains(errb, "observation") {
		t.Fatalf("should note it is an observation: %s", errb)
	}
	if !strings.Contains(out, "HKCategoryValueSleepAnalysis") && !strings.Contains(out, "SleepAnalysis") {
		t.Fatalf("episodes -kind SleepAnalysis should still return the records:\n%s", out)
	}
}

func TestEmptyResultShowsKindSpan(t *testing.T) {
	root := appleRoot(t)
	code, out, errb := runCmd(t, "-root", root, "observations", "-kind", "BodyMass", "-from", "2026-03-01", "-to", "2026-03-31")
	if code != 0 {
		t.Fatalf("%s", errb)
	}
	if !strings.Contains(out, "no matching rows") {
		t.Fatalf("expected empty hint:\n%s", out)
	}
	if !strings.Contains(out, "HKQuantityTypeIdentifierBodyMass") || !strings.Contains(out, "n=") {
		t.Fatalf("expected kind span:\n%s", out)
	}
	if !strings.Contains(out, "2025-09-11") {
		t.Fatalf("BodyMass range should mention the fixture date:\n%s", out)
	}
}

func TestRestingHeartRateUnitAndWatch(t *testing.T) {
	root := appleRoot(t)
	code, out, errb := runCmd(t, "-root", root, "observations", "-kind", "RestingHeartRate")
	if code != 0 {
		t.Fatalf("%s", errb)
	}
	if !strings.Contains(out, "count/min") {
		t.Fatalf("RestingHeartRate unit:\n%s", out)
	}
	code, out, errb = runCmd(t, "-root", root, "observations", "-on", "2025-09-11", "-source", "Apple Watch")
	if code != 0 {
		t.Fatalf("%s", errb)
	}
	if !strings.Contains(out, "Apple Watch") {
		t.Fatalf("Watch rows on 2025-09-11:\n%s", out)
	}
}

func TestSummaryFlag(t *testing.T) {
	root := appleRoot(t)
	code, out, errb := runCmd(t, "-root", root, "observations", "-kind", "StepCount", "-summary")
	if code != 0 {
		t.Fatalf("%s", errb)
	}
	for _, want := range []string{"kind ", "n ", "from ", "to ", "Apple Watch", "sum="} {
		if !strings.Contains(out, want) {
			t.Fatalf("summary missing %q:\n%s", want, out)
		}
	}
}

func TestStatsKindsSources(t *testing.T) {
	root := appleRoot(t)
	out := runOK(t, "-root", root, "stats")
	for _, want := range []string{"events", "blobs", "observations", "episodes", "from", "to"} {
		if !strings.Contains(out, want) {
			t.Fatalf("stats missing %s:\n%s", want, out)
		}
	}
	kinds := runOK(t, "-root", root, "kinds", "-count")
	if !strings.Contains(kinds, "HKQuantityTypeIdentifierStepCount") {
		t.Fatalf("kinds: %s", kinds)
	}
	sources := runOK(t, "-root", root, "sources")
	if !strings.Contains(sources, "Apple Watch") {
		t.Fatalf("sources: %s", sources)
	}
	// At least one other source besides the Watch (name is fixture-defined).
	if strings.Count(strings.TrimSpace(sources), "\n") < 1 {
		t.Fatalf("expected multiple sources:\n%s", sources)
	}
	eps := runOK(t, "-root", root, "episodes", "-kind", "Running")
	if !strings.Contains(eps, "Running") {
		t.Fatalf("episodes: %s", eps)
	}
}
