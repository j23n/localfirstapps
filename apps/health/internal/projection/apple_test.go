package projection

import (
	"archive/zip"
	"database/sql"
	"io"
	"os"
	"path/filepath"
	"testing"

	"archive/internal/adapters/apple"
	"archive/internal/blobs"
	"archive/internal/event"
	"archive/internal/log"
	"archive/internal/uuid"
)

func zipXML(t *testing.T, xmlPath, dest string) {
	t.Helper()
	f, err := os.Create(dest)
	if err != nil {
		t.Fatal(err)
	}
	zw := zip.NewWriter(f)
	w, err := zw.Create("apple_health_export/export.xml")
	if err != nil {
		t.Fatal(err)
	}
	src, err := os.Open(xmlPath)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := io.Copy(w, src); err != nil {
		t.Fatal(err)
	}
	src.Close()
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	if err := f.Close(); err != nil {
		t.Fatal(err)
	}
}

func importFile(t *testing.T, root, dev, path string) {
	t.Helper()
	f, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	hash, size, _, err := blobs.Put(root, f)
	f.Close()
	if err != nil {
		t.Fatal(err)
	}
	ok, err := log.HasBlobImport(root, hash)
	if err != nil {
		t.Fatal(err)
	}
	if ok {
		return
	}
	id, err := uuid.NewV7()
	if err != nil {
		t.Fatal(err)
	}
	body := []byte(`{"sha256":"` + hash + `","size":` + itoa(size) + `,"name":"` + filepath.Base(path) + `","kind":"apple"}`)
	ev := event.Event{ID: id, TS: event.NowUTC(), Dev: dev, Type: event.TypeBlobImport, Body: body}
	if err := log.Append(root, ev); err != nil {
		t.Fatal(err)
	}
}

func itoa(n int64) string {
	if n == 0 {
		return "0"
	}
	var b [20]byte
	i := len(b)
	for n > 0 {
		i--
		b[i] = byte('0' + n%10)
		n /= 10
	}
	return string(b[i:])
}

func TestAppleOverlapDedup(t *testing.T) {
	root := t.TempDir()
	a := filepath.Join(t.TempDir(), "a.zip")
	b := filepath.Join(t.TempDir(), "b.zip")
	zipXML(t, filepath.Join("..", "..", "testdata", "apple", "export.xml"), a)
	zipXML(t, filepath.Join("..", "..", "testdata", "apple", "export-overlap.xml"), b)
	importFile(t, root, "apple", a)
	importFile(t, root, "apple", b)
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	db, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()

	want := unionGroupCount(t)
	var got int
	if err := db.QueryRow(`SELECT COUNT(*) FROM observations`).Scan(&got); err != nil {
		t.Fatal(err)
	}
	if got != want {
		t.Fatalf("observations=%d want union-of-group-counts %d", got, want)
	}

	// source_precedence populated; Watch preferred over iPhone for identical windows
	var watchRank, phoneRank int
	if err := db.QueryRow(`SELECT rank FROM source_precedence WHERE source_name = 'Apple Watch'`).Scan(&watchRank); err != nil {
		t.Fatal(err)
	}
	// Rank is looked up by sourceName (iPhone), not by parsing the Health device field.
	if err := db.QueryRow(`SELECT rank FROM source_precedence WHERE source_name = 'iPhone'`).Scan(&phoneRank); err != nil {
		t.Fatal(err)
	}
	if watchRank >= phoneRank {
		t.Fatalf("watch rank %d should beat iPhone %d", watchRank, phoneRank)
	}
}

func unionGroupCount(t *testing.T) int {
	t.Helper()
	a := keyCounts(t, "export.xml")
	b := keyCounts(t, "export-overlap.xml")
	n := 0
	seen := map[string]bool{}
	for k, ca := range a {
		cb := b[k]
		if ca < cb {
			n += cb
		} else {
			n += ca
		}
		seen[k] = true
	}
	for k, cb := range b {
		if !seen[k] {
			n += cb
		}
	}
	return n
}

func keyCounts(t *testing.T, name string) map[string]int {
	t.Helper()
	f, err := os.Open(filepath.Join("..", "..", "testdata", "apple", name))
	if err != nil {
		t.Fatal(err)
	}
	_, obs, _, err := apple.Collect(f)
	f.Close()
	if err != nil {
		t.Fatal(err)
	}
	m := map[string]int{}
	for _, o := range obs {
		m[o.DedupKey]++
	}
	return m
}

func TestRealCollisionGroups(t *testing.T) {
	f, err := os.Open(filepath.Join("..", "..", "testdata", "apple", "export.xml"))
	if err != nil {
		t.Fatal(err)
	}
	_, obs, _, err := apple.Collect(f)
	f.Close()
	if err != nil {
		t.Fatal(err)
	}

	var hr0, hr2 apple.Observation
	var watchSteps []apple.Observation
	for _, o := range obs {
		if o.Kind == "HKQuantityTypeIdentifierHeartRate" && o.Value == "68" && o.Start == "2025-09-12T18:04:07.000000000Z" {
			for _, m := range o.Metadata {
				if m.Key == "HKMetadataKeyHeartRateMotionContext" && m.Value == "0" {
					hr0 = o
				}
				if m.Key == "HKMetadataKeyHeartRateMotionContext" && m.Value == "2" {
					hr2 = o
				}
			}
		}
		if o.Kind == "HKQuantityTypeIdentifierStepCount" && o.Source == "Apple Watch" && o.Value == "198" &&
			o.Start == "2025-09-16T17:45:01.000000000Z" {
			watchSteps = append(watchSteps, o)
		}
	}
	if hr0.DedupKey == "" || hr2.DedupKey == "" {
		t.Fatal("fixture missing the real HeartRate 68 pair")
	}
	if hr0.DedupKey == hr2.DedupKey {
		t.Fatal("HeartRate motion 0 vs 2 still collide")
	}
	if len(watchSteps) != 2 {
		t.Fatalf("want both identical Watch StepCount 198 copies, got %d", len(watchSteps))
	}
	if watchSteps[0].DedupKey != watchSteps[1].DedupKey {
		t.Fatal("identical StepCount copies must share a group key")
	}

	root := t.TempDir()
	z := filepath.Join(t.TempDir(), "export.zip")
	zipXML(t, filepath.Join("..", "..", "testdata", "apple", "export.xml"), z)
	importFile(t, root, "apple", z)
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	countKey := func() int {
		t.Helper()
		db, err := Open(root)
		if err != nil {
			t.Fatal(err)
		}
		defer db.Close()
		var n int
		if err := db.QueryRow(`SELECT COUNT(*) FROM observations WHERE dedup_key = ?`, watchSteps[0].DedupKey).Scan(&n); err != nil {
			t.Fatal(err)
		}
		return n
	}
	if n := countKey(); n != 2 {
		t.Fatalf("group size in db=%d want 2", n)
	}
	// Overlap fixture has one copy; rebuild with both must stay at 2.
	z2 := filepath.Join(t.TempDir(), "overlap.zip")
	zipXML(t, filepath.Join("..", "..", "testdata", "apple", "export-overlap.xml"), z2)
	importFile(t, root, "apple", z2)
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	if n := countKey(); n != 2 {
		t.Fatalf("after overlap import group size=%d want 2", n)
	}
}

func TestAppleRebuildDeterminism(t *testing.T) {
	root := t.TempDir()
	z := filepath.Join(t.TempDir(), "export.zip")
	zipXML(t, filepath.Join("..", "..", "testdata", "apple", "export.xml"), z)
	importFile(t, root, "apple", z)
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	a := fileSHA(t, DBPath(root))
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	b := fileSHA(t, DBPath(root))
	if a != b {
		t.Fatalf("rebuilds differ: %s vs %s", a, b)
	}
}

func TestAppleIdempotentImport(t *testing.T) {
	root := t.TempDir()
	z := filepath.Join(t.TempDir(), "export.zip")
	zipXML(t, filepath.Join("..", "..", "testdata", "apple", "export.xml"), z)
	importFile(t, root, "apple", z)
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	h1 := fileSHA(t, DBPath(root))
	n1 := eventCount(t, root)
	importFile(t, root, "apple", z)
	n2 := eventCount(t, root)
	if n1 != n2 {
		t.Fatalf("re-import added events: %d -> %d", n1, n2)
	}
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	h2 := fileSHA(t, DBPath(root))
	if h1 != h2 {
		t.Fatal("re-import changed the database")
	}
}

func eventCount(t *testing.T, root string) int {
	t.Helper()
	evs, err := log.ReadAll(root)
	if err != nil {
		t.Fatal(err)
	}
	return len(evs)
}

func TestObservationProvenance(t *testing.T) {
	root := t.TempDir()
	z := filepath.Join(t.TempDir(), "export.zip")
	zipXML(t, filepath.Join("..", "..", "testdata", "apple", "export.xml"), z)
	importFile(t, root, "apple", z)
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	db, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var missing int
	if err := db.QueryRow(`SELECT COUNT(*) FROM observations WHERE blob_sha256 IS NULL OR blob_sha256 = '' OR event_id IS NULL OR event_id = ''`).Scan(&missing); err != nil {
		t.Fatal(err)
	}
	if missing != 0 {
		t.Fatalf("observations without provenance: %d", missing)
	}
	evs, err := log.ReadAll(root)
	if err != nil {
		t.Fatal(err)
	}
	var imp event.Event
	for _, ev := range evs {
		if ev.Type == event.TypeBlobImport {
			imp = ev
			break
		}
	}
	if imp.ID == "" {
		t.Fatal("no blob_import")
	}
	hash, ok := imp.BlobSHA256()
	if !ok {
		t.Fatal("blob_import missing sha256")
	}
	var n int
	if err := db.QueryRow(`SELECT COUNT(*) FROM observations WHERE blob_sha256 = ? AND event_id = ?`, hash, imp.ID).Scan(&n); err != nil {
		t.Fatal(err)
	}
	if n == 0 {
		t.Fatal("no observation points at the blob_import")
	}
}

func TestAppleWorkoutAndSummary(t *testing.T) {
	root := t.TempDir()
	z := filepath.Join(t.TempDir(), "export.zip")
	zipXML(t, filepath.Join("..", "..", "testdata", "apple", "export.xml"), z)
	importFile(t, root, "apple", z)
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	db, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var runs, summaries, corrs int
	mustCount(t, db, `SELECT COUNT(*) FROM episodes WHERE kind = 'HKWorkoutActivityTypeRunning'`, &runs)
	mustCount(t, db, `SELECT COUNT(*) FROM episodes WHERE kind = 'ActivitySummary'`, &summaries)
	mustCount(t, db, `SELECT COUNT(*) FROM episodes WHERE kind LIKE 'HKCorrelation%'`, &corrs)
	if runs != 1 || summaries != 12 || corrs != 0 {
		t.Fatalf("runs=%d summaries=%d corrs=%d", runs, summaries, corrs)
	}
	var withStats int
	mustCount(t, db, `SELECT COUNT(*) FROM episodes WHERE body LIKE '%statistics%'`, &withStats)
	if withStats < 1 {
		t.Fatal("workout statistics dropped")
	}
	var withRoute int
	mustCount(t, db, `SELECT COUNT(*) FROM episodes WHERE body LIKE '%workout-routes%'`, &withRoute)
	if withRoute < 1 {
		t.Fatal("workout route path not recorded")
	}
}

func mustCount(t *testing.T, db *sql.DB, q string, dest *int) {
	t.Helper()
	if err := db.QueryRow(q).Scan(dest); err != nil {
		t.Fatal(err)
	}
}
