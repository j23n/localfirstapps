package projection

import (
	"path/filepath"
	"testing"
	"time"
)

func appleDB(t *testing.T) string {
	t.Helper()
	root := t.TempDir()
	z := filepath.Join(t.TempDir(), "export.zip")
	zipXML(t, filepath.Join("..", "..", "testdata", "apple", "export.xml"), z)
	importFile(t, root, "apple", z)
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	return root
}

func TestStepCountKnownMissingDays(t *testing.T) {
	root := appleDB(t)
	db, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()

	loc := time.FixedZone("+0200", 2*3600)
	g, err := ObservationGaps(db, "HKQuantityTypeIdentifierStepCount", loc)
	if err != nil {
		t.Fatal(err)
	}
	present := map[string]bool{}
	for _, d := range g.Present {
		present[d] = true
	}
	// Verbatim fixture days (local +0200). 2025-08-11 sits between
	// the August cluster and the September cluster.
	for _, d := range []string{"2025-08-08", "2025-08-09", "2025-08-10", "2025-09-11", "2025-09-12"} {
		if !present[d] {
			t.Fatalf("expected present %s; got %v", d, g.Present)
		}
	}
	if present["2025-08-11"] {
		t.Fatal("2025-08-11 should be a gap")
	}
	found := false
	for _, d := range g.Missing {
		if d == "2025-08-11" {
			found = true
			break
		}
	}
	if !found {
		t.Fatalf("missing should include 2025-08-11; first=%s last=%s n=%d", g.First, g.Last, g.MissingN())
	}
	if g.MissingN() == 0 {
		t.Fatal("fixture is sparse; expected missing days")
	}
}

func TestMultiSourceKindsNamePreferred(t *testing.T) {
	root := appleDB(t)
	db, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	cat, err := KindCatalog(db)
	if err != nil {
		t.Fatal(err)
	}
	multi := MultiSourceKinds(cat)
	if len(multi) == 0 {
		t.Fatal("fixture should have multi-source kinds")
	}
	for _, k := range multi {
		if k.Preferred == "" {
			t.Fatalf("%s: missing preferred", k.Kind)
		}
		if k.Preferred != "Apple Watch" && k.Preferred != "iPhone" {
			t.Fatalf("%s preferred %q", k.Kind, k.Preferred)
		}
		t.Logf("%s sources=%v preferred=%s overlaps=%v", ShortKind(k.Kind), k.Sources, k.Preferred, k.Overlaps)
	}
	steps := FindKind(cat, "HKQuantityTypeIdentifierStepCount")
	if len(steps.Sources) < 2 {
		t.Fatal("fixture StepCount should have Watch and phone")
	}
	if steps.Overlaps {
		t.Fatal("fixture StepCount Watch and phone do not share a local day")
	}
}

func TestOverlapsWhenTwoSourcesShareALocalDay(t *testing.T) {
	t.Setenv("ARCHIVE_TZ", "+0200")
	root := appleDB(t)
	db, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	// Different UTC dates, same local +0200 day (2025-09-21).
	_, err = db.Exec(`INSERT INTO observations (dedup_key, n, kind, source, start_ts, end_ts, start_offset, metadata, value, unit)
		VALUES ('overlap-watch', 1, 'HKQuantityTypeIdentifierStepCount', 'Apple Watch',
			'2025-09-20T22:30:00.000000000Z', '2025-09-20T22:31:00.000000000Z', '+0200', '{}', '10', 'count')`)
	if err != nil {
		t.Fatal(err)
	}
	_, err = db.Exec(`INSERT INTO observations (dedup_key, n, kind, source, start_ts, end_ts, start_offset, metadata, value, unit)
		VALUES ('overlap-phone', 1, 'HKQuantityTypeIdentifierStepCount', 'iPhone',
			'2025-09-21T10:00:00.000000000Z', '2025-09-21T10:01:00.000000000Z', '+0200', '{}', '20', 'count')`)
	if err != nil {
		t.Fatal(err)
	}
	cat, err := KindCatalog(db)
	if err != nil {
		t.Fatal(err)
	}
	steps := FindKind(cat, "HKQuantityTypeIdentifierStepCount")
	if !steps.Overlaps {
		t.Fatal("Watch 22:30Z and phone 10:00Z next UTC day are the same local +0200 day")
	}
}

func TestOverallGapsNonEmptySpan(t *testing.T) {
	root := appleDB(t)
	db, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	loc := time.FixedZone("+0200", 2*3600)
	g, err := OverallGaps(db, loc)
	if err != nil {
		t.Fatal(err)
	}
	if g.First == "" || g.Last == "" {
		t.Fatal("empty overall span")
	}
	if g.MissingN()+g.PresentN() != g.SpanDays() {
		t.Fatalf("present %d + missing %d != span %d", g.PresentN(), g.MissingN(), g.SpanDays())
	}
}
