package projection

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"testing"

	"archive/internal/blobs"
	"archive/internal/event"
	"archive/internal/log"
	"archive/internal/uuid"
)

func copyTree(t *testing.T, src, dst string) {
	t.Helper()
	err := filepath.Walk(src, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(src, path)
		if err != nil {
			return err
		}
		target := filepath.Join(dst, rel)
		if info.IsDir() {
			return os.MkdirAll(target, 0o755)
		}
		in, err := os.Open(path)
		if err != nil {
			return err
		}
		defer in.Close()
		if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
			return err
		}
		out, err := os.Create(target)
		if err != nil {
			return err
		}
		_, copyErr := io.Copy(out, in)
		closeErr := out.Close()
		if copyErr != nil {
			return copyErr
		}
		return closeErr
	})
	if err != nil {
		t.Fatal(err)
	}
}

func fileSHA(t *testing.T, path string) string {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

func TestRebuildFixture(t *testing.T) {
	root := t.TempDir()
	copyTree(t, filepath.Join("..", "..", "testdata", "m0"), root)
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	db, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	all, err := Query(db, Filter{})
	if err != nil {
		t.Fatal(err)
	}
	if len(all) != 6 {
		t.Fatalf("all=%d", len(all))
	}
	cur, err := Query(db, Filter{Current: true})
	if err != nil {
		t.Fatal(err)
	}
	// 6 events minus superseded note, minus retracted note = 4 current
	// (supersede and retract events themselves remain)
	ids := map[string]bool{}
	for _, ev := range cur {
		ids[ev.ID] = true
	}
	if ids["01900000-0000-7000-8000-000000000002"] {
		t.Fatal("superseded note still current")
	}
	if ids["01900000-0000-7000-8000-000000000004"] {
		t.Fatal("retracted note still current")
	}
	if !ids["01900000-0000-7000-8000-000000000001"] || !ids["01900000-0000-7000-8000-000000000003"] {
		t.Fatalf("current ids: %v", ids)
	}
	notes, err := Query(db, Filter{Type: event.TypeNote})
	if err != nil {
		t.Fatal(err)
	}
	if len(notes) != 3 {
		t.Fatalf("notes=%d", len(notes))
	}
	var n int
	if err := db.QueryRow(`SELECT COUNT(*) FROM blobs`).Scan(&n); err != nil {
		t.Fatal(err)
	}
	if n != 1 {
		t.Fatalf("blobs=%d", n)
	}
}

func TestRetractedBlobImportNotProjected(t *testing.T) {
	root := t.TempDir()
	src := filepath.Join("..", "..", "testdata", "apple", "export.xml")
	f, err := os.Open(src)
	if err != nil {
		t.Fatal(err)
	}
	hash, size, _, err := blobs.Put(root, f)
	f.Close()
	if err != nil {
		t.Fatal(err)
	}
	id, err := uuid.NewV7()
	if err != nil {
		t.Fatal(err)
	}
	body, err := json.Marshal(map[string]any{"sha256": hash, "size": size, "name": "export.xml", "kind": "apple"})
	if err != nil {
		t.Fatal(err)
	}
	imp := event.Event{ID: id, TS: event.NowUTC(), Dev: "apple", Type: event.TypeBlobImport, Body: body}
	if err := log.Append(root, imp); err != nil {
		t.Fatal(err)
	}
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	db, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	before, err := TableCounts(db)
	db.Close()
	if err != nil {
		t.Fatal(err)
	}
	if before.Observations == 0 {
		t.Fatal("expected observations from fixture blob")
	}
	if before.Blobs != 1 {
		t.Fatalf("blobs=%d", before.Blobs)
	}

	rid, err := uuid.NewV7()
	if err != nil {
		t.Fatal(err)
	}
	retract := event.Event{
		ID:   rid,
		TS:   event.NowUTC(),
		Dev:  "apple",
		Type: event.TypeRetract,
		Body: json.RawMessage(`{"target":"` + id + `"}`),
	}
	if err := log.Append(root, retract); err != nil {
		t.Fatal(err)
	}
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	db, err = Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	after, err := TableCounts(db)
	if err != nil {
		t.Fatal(err)
	}
	if after.Observations != 0 {
		t.Fatalf("observations after retract=%d want 0", after.Observations)
	}
	if after.Blobs != 1 {
		t.Fatalf("blob row dropped after retract: blobs=%d", after.Blobs)
	}
	var nFromBlob int
	if err := db.QueryRow(`SELECT COUNT(*) FROM observations WHERE blob_sha256 = ?`, hash).Scan(&nFromBlob); err != nil {
		t.Fatal(err)
	}
	if nFromBlob != 0 {
		t.Fatalf("observations still linked to retracted blob: %d", nFromBlob)
	}
}

func TestRebuildDirAndDBMode(t *testing.T) {
	root := t.TempDir()
	copyTree(t, filepath.Join("..", "..", "testdata", "m0"), root)
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	info, err := os.Stat(filepath.Dir(DBPath(root)))
	if err != nil {
		t.Fatal(err)
	}
	if info.Mode().Perm() != 0o700 {
		t.Fatalf("derived dir mode %o", info.Mode().Perm())
	}
	dbInfo, err := os.Stat(DBPath(root))
	if err != nil {
		t.Fatal(err)
	}
	if dbInfo.Mode().Perm() != 0o600 {
		t.Fatalf("archive.db mode %o", dbInfo.Mode().Perm())
	}
}

func TestRebuildDeterminism(t *testing.T) {
	root := t.TempDir()
	copyTree(t, filepath.Join("..", "..", "testdata", "m0"), root)
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
