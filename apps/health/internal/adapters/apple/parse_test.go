package apple

import (
	"bytes"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"testing"
	"time"
)

func fixture(name string) string {
	return filepath.Join("..", "..", "..", "testdata", "apple", name)
}

func TestCorrelationDTDComment(t *testing.T) {
	b, err := os.ReadFile(fixture("export.xml"))
	if err != nil {
		t.Fatal(err)
	}
	if !HasCorrelationDTDComment(b) {
		t.Fatal("fixture DTD lacks the nested-Record duplicate comment; confirm before skipping children")
	}
}

func TestGoldenExport(t *testing.T) {
	f, err := os.Open(fixture("export.xml"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	got, err := GoldenBytes(f)
	if err != nil {
		t.Fatal(err)
	}
	wantPath := fixture("golden.ndjson")
	if os.Getenv("UPDATE_GOLDEN") != "" {
		if err := os.WriteFile(wantPath, got, 0o644); err != nil {
			t.Fatal(err)
		}
	}
	want, err := os.ReadFile(wantPath)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("golden mismatch: got %d bytes, want %d\nfirst diff at %d", len(got), len(want), firstDiff(got, want))
	}
}

func firstDiff(a, b []byte) int {
	n := len(a)
	if len(b) < n {
		n = len(b)
	}
	for i := 0; i < n; i++ {
		if a[i] != b[i] {
			return i
		}
	}
	return n
}

func TestStreamSkipsCorrelationChildren(t *testing.T) {
	f, err := os.Open(fixture("export.xml"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	_, obs, _, err := Collect(f)
	if err != nil {
		t.Fatal(err)
	}
	var sys, dia int
	for _, o := range obs {
		switch o.Kind {
		case "HKQuantityTypeIdentifierBloodPressureSystolic":
			sys++
		case "HKQuantityTypeIdentifierBloodPressureDiastolic":
			dia++
		}
	}
	raw, err := os.ReadFile(fixture("export.xml"))
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Count(raw, []byte("<Correlation")) == 0 {
		if sys != 0 || dia != 0 {
			t.Fatalf("no Correlation in fixture but BP observations sys=%d dia=%d", sys, dia)
		}
		t.Log("fixture has no Correlation elements (true of the full export too)")
		return
	}
	rawSys := bytes.Count(raw, []byte(`type="HKQuantityTypeIdentifierBloodPressureSystolic"`))
	if sys*2 != rawSys {
		t.Fatalf("systolic observations=%d xml type attrs=%d (want observations = half of attrs: top-level + nested)", sys, rawSys)
	}
}

func TestUTCAndOffsetPreserved(t *testing.T) {
	f, err := os.Open(fixture("export.xml"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	_, obs, _, err := Collect(f)
	if err != nil {
		t.Fatal(err)
	}
	if len(obs) == 0 {
		t.Fatal("no observations")
	}
	for _, o := range obs {
		if !strings.HasSuffix(o.Start, "Z") || !strings.HasSuffix(o.End, "Z") {
			t.Fatalf("not UTC: %+v", o)
		}
		if len(o.StartOffset) != 5 || (o.StartOffset[0] != '+' && o.StartOffset[0] != '-') {
			t.Fatalf("offset %q on %s", o.StartOffset, o.Kind)
		}
		if o.Kind == "" {
			t.Fatal("empty kind")
		}
		if o.DedupKey == "" {
			t.Fatal("empty dedup_key")
		}
	}
}

func TestActivitySummaryExportDateOffset(t *testing.T) {
	f, err := os.Open(fixture("export.xml"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	hdr, _, eps, err := Collect(f)
	if err != nil {
		t.Fatal(err)
	}
	if hdr.ExportDate == "" {
		t.Fatal("fixture missing ExportDate")
	}
	_, exportOff, err := ParseTime(hdr.ExportDate)
	if err != nil {
		t.Fatal(err)
	}
	ref, err := time.Parse(AppleTimeLayout, "2000-01-01 00:00:00 "+exportOff)
	if err != nil {
		t.Fatal(err)
	}
	loc := ref.Location()
	var n int
	for _, e := range eps {
		if e.Kind != "ActivitySummary" {
			continue
		}
		n++
		if e.StartOffset != exportOff || e.EndOffset != exportOff {
			t.Fatalf("offsets start=%q end=%q want %q", e.StartOffset, e.EndOffset, exportOff)
		}
		start, err := time.Parse("2006-01-02T15:04:05.000000000Z", e.Start)
		if err != nil {
			t.Fatal(err)
		}
		local := start.In(loc)
		if local.Hour() != 0 || local.Minute() != 0 || local.Second() != 0 {
			t.Fatalf("start %s is not local midnight in %s (got %s)", e.Start, exportOff, local)
		}
		end, err := time.Parse("2006-01-02T15:04:05.000000000Z", e.End)
		if err != nil {
			t.Fatal(err)
		}
		localEnd := end.In(loc)
		if localEnd.Hour() != 23 || localEnd.Minute() != 59 || localEnd.Second() != 59 {
			t.Fatalf("end %s is not local 23:59:59 in %s (got %s)", e.End, exportOff, localEnd)
		}
	}
	if n == 0 {
		t.Fatal("no ActivitySummary in fixture")
	}
}

func TestActivitySummaryDedupKeyIsDateOnly(t *testing.T) {
	const xml = `<?xml version="1.0"?>
<HealthData locale="en_US">
  <ExportDate value="2026-09-08 14:05:33 +0200"/>
  <Me/>
  <ActivitySummary dateComponents="2025-09-11" activeEnergyBurned="100"/>
  <ActivitySummary dateComponents="2025-09-11" activeEnergyBurned="999"/>
</HealthData>`
	_, _, eps, err := Collect(strings.NewReader(xml))
	if err != nil {
		t.Fatal(err)
	}
	if len(eps) != 2 {
		t.Fatalf("eps=%d", len(eps))
	}
	if eps[0].DedupKey != eps[1].DedupKey {
		t.Fatal("revised energy produced different keys")
	}
	want := DedupKey("ActivitySummary", "", "2025-09-11", "2025-09-11", "", nil)
	if eps[0].DedupKey != want {
		t.Fatalf("key=%s want date-only %s", eps[0].DedupKey, want)
	}
}

func TestActivitySummaryMissingExportDateFallback(t *testing.T) {
	const xml = `<?xml version="1.0"?>
<HealthData locale="en_US">
  <Me/>
  <ActivitySummary dateComponents="2025-09-11" activeEnergyBurned="238.706"/>
</HealthData>`
	_, _, eps, err := Collect(strings.NewReader(xml))
	if err != nil {
		t.Fatal(err)
	}
	if len(eps) != 1 {
		t.Fatalf("eps=%d", len(eps))
	}
	if eps[0].Start != "2025-09-11T00:00:00.000000000Z" || eps[0].End != "2025-09-11T23:59:59.000000000Z" {
		t.Fatalf("fallback times %+v", eps[0])
	}
	if eps[0].StartOffset != "" || eps[0].EndOffset != "" {
		t.Fatalf("unexpected offsets %+v", eps[0])
	}
}

func TestReadGenericBadDates(t *testing.T) {
	bad := `<?xml version="1.0"?>
<HealthData locale="en_US">
  <ClinicalRecord startDate="not-a-date" endDate="2024-03-01 08:00:00 -0800"/>
</HealthData>`
	if _, _, _, err := Collect(strings.NewReader(bad)); err == nil {
		t.Fatal("expected error for unparseable startDate")
	}
	missing := `<?xml version="1.0"?>
<HealthData locale="en_US">
  <ClinicalRecord type="HKUnknown"/>
</HealthData>`
	_, _, eps, err := Collect(strings.NewReader(missing))
	if err != nil {
		t.Fatal(err)
	}
	if len(eps) != 1 {
		t.Fatalf("eps=%d", len(eps))
	}
	if eps[0].Start != "1970-01-01T00:00:00.000000000Z" || eps[0].End != eps[0].Start {
		t.Fatalf("Start/End inconsistent: %+v", eps[0])
	}
}

func TestNoTypeSwitchDropsUnknown(t *testing.T) {
	const xml = `<?xml version="1.0"?>
<HealthData locale="en_US">
  <ExportDate value="2024-03-20 12:00:00 -0700"/>
  <Me HKCharacteristicTypeIdentifierDateOfBirth="1990-01-15"/>
  <Record type="HKQuantityTypeIdentifierSomethingInventedTomorrow" sourceName="Watch" startDate="2024-03-01 08:00:00 -0800" endDate="2024-03-01 08:00:00 -0800" value="1"/>
</HealthData>`
	_, obs, _, err := Collect(strings.NewReader(xml))
	if err != nil {
		t.Fatal(err)
	}
	if len(obs) != 1 || obs[0].Kind != "HKQuantityTypeIdentifierSomethingInventedTomorrow" {
		t.Fatalf("%+v", obs)
	}
}

func TestBoundedMemory(t *testing.T) {
	const n = 200_000
	pr, pw := io.Pipe()
	go func() {
		defer pw.Close()
		io.WriteString(pw, `<?xml version="1.0"?><HealthData locale="en_US"><ExportDate value="2024-03-20 12:00:00 -0700"/><Me HKCharacteristicTypeIdentifierDateOfBirth="1990-01-15"/>`)
		for i := 0; i < n; i++ {
			d := 1 + (i % 28)
			h := i % 24
			v := i % 200
			io.WriteString(pw, `<Record type="HKQuantityTypeIdentifierHeartRate" sourceName="Watch"`)
			io.WriteString(pw, ` startDate="2024-03-`)
			if d < 10 {
				io.WriteString(pw, "0")
			}
			io.WriteString(pw, itoa(d))
			io.WriteString(pw, ` `)
			if h < 10 {
				io.WriteString(pw, "0")
			}
			io.WriteString(pw, itoa(h))
			io.WriteString(pw, `:00:00 -0800" endDate="2024-03-`)
			if d < 10 {
				io.WriteString(pw, "0")
			}
			io.WriteString(pw, itoa(d))
			io.WriteString(pw, ` `)
			if h < 10 {
				io.WriteString(pw, "0")
			}
			io.WriteString(pw, itoa(h))
			io.WriteString(pw, `:00:00 -0800" value="`)
			io.WriteString(pw, itoa(v))
			io.WriteString(pw, `"/>`)
		}
		io.WriteString(pw, `</HealthData>`)
	}()

	runtime.GC()
	var before runtime.MemStats
	runtime.ReadMemStats(&before)

	var got int
	_, err := Stream(pr, Handler{
		Observation: func(Observation) error {
			got++
			return nil
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if got != n {
		t.Fatalf("got %d want %d", got, n)
	}

	runtime.GC()
	var after runtime.MemStats
	runtime.ReadMemStats(&after)
	var used uint64
	if after.HeapAlloc > before.HeapAlloc {
		used = after.HeapAlloc - before.HeapAlloc
	}
	const capBytes = 64 << 20
	if used > capBytes {
		t.Fatalf("heap grew by %d bytes streaming %d records (cap %d) — parser is not bounded", used, n, capBytes)
	}
}

func itoa(i int) string {
	if i == 0 {
		return "0"
	}
	var b [12]byte
	p := len(b)
	for i > 0 {
		p--
		b[p] = byte('0' + i%10)
		i /= 10
	}
	return string(b[p:])
}

func TestFullExport(t *testing.T) {
	path := os.Getenv("ARCHIVE_TEST_FULL_EXPORT")
	if path == "" {
		t.Skip("set ARCHIVE_TEST_FULL_EXPORT to run against a real export")
	}
	if _, err := os.Stat(path); err != nil {
		t.Skipf("ARCHIVE_TEST_FULL_EXPORT=%s not found: %v", path, err)
	}
	raw, err := os.ReadFile(path)
	if err != nil {
		// zip path — OpenXML still works; DTD comment check needs the xml
		raw = nil
		_ = err
	}
	if raw != nil && !HasCorrelationDTDComment(raw) {
		t.Fatal("full export DTD missing Correlation duplicate-record comment")
	}
	if raw != nil {
		t.Logf("Correlation start tags in XML: %d", bytes.Count(raw, []byte("<Correlation")))
	}

	rc, err := OpenXML(path)
	if err != nil {
		t.Fatal(err)
	}
	defer rc.Close()

	obsKeys := map[string]int{}
	kinds := map[string]int{}
	sources := map[string]int{}
	epKinds := map[string]int{}
	var n, emptyKind, badTS, corrEps int
	hdr, err := Stream(rc, Handler{
		Observation: func(o Observation) error {
			n++
			if o.Kind == "" {
				emptyKind++
			}
			if o.Start == "" || !strings.HasSuffix(o.Start, "Z") {
				badTS++
			}
			obsKeys[o.DedupKey]++
			kinds[o.Kind]++
			sources[o.Source]++
			return nil
		},
		Episode: func(e Episode) error {
			epKinds[e.Kind]++
			if strings.Contains(e.Kind, "Correlation") || strings.HasPrefix(e.Kind, "HKCorrelation") {
				corrEps++
			}
			return nil
		},
	})
	if err != nil {
		t.Fatalf("parse error: %v", err)
	}
	if emptyKind > 0 {
		t.Fatalf("%d records with empty kind", emptyKind)
	}
	if badTS > 0 {
		t.Fatalf("%d records with unparseable/non-UTC timestamps", badTS)
	}

	var dups int
	for _, c := range obsKeys {
		if c > 1 {
			dups++
		}
	}
	t.Logf("header locale=%q exportDate=%q me=%v", hdr.Locale, hdr.ExportDate, hdr.Me)
	t.Logf("observations=%d kinds=%d sources=%v", n, len(kinds), sources)
	t.Logf("episodes=%v", epKinds)
	t.Logf("dedup_key collisions (prescribed key, single export): %d", dups)
	t.Logf("correlation episodes: %d", corrEps)
	t.Log("HK types:")
	var names []string
	for k := range kinds {
		names = append(names, k)
	}
	sort.Strings(names)
	for _, k := range names {
		t.Logf("  %8d  %s", kinds[k], k)
	}
	med := 0
	for k, c := range kinds {
		lk := strings.ToLower(k)
		if strings.Contains(lk, "medication") || strings.Contains(lk, "dose") || strings.Contains(lk, "clinical") {
			t.Logf("medication-related kind %s count=%d", k, c)
			med += c
		}
	}
	if v := hdr.Me["HKCharacteristicTypeIdentifierCardioFitnessMedicationsUse"]; v != "" {
		t.Logf("Me.CardioFitnessMedicationsUse=%q", v)
	}
	t.Logf("medication record count=%d", med)
}
