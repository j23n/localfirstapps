package portable

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"

	"archive/internal/blobs"
	"archive/internal/event"
	"archive/internal/log"
	"archive/internal/projection"
	"archive/internal/version"
)

const helloSHA = "a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d"

func TestExportDeterminism(t *testing.T) {
	root := filepath.Join("..", "..", "testdata", "m0")
	a := t.TempDir()
	b := t.TempDir()
	ma, err := Export(root, a)
	if err != nil {
		t.Fatal(err)
	}
	mb, err := Export(root, b)
	if err != nil {
		t.Fatal(err)
	}
	if ma.ExportedAt == "" || mb.ExportedAt == "" {
		t.Fatal("missing exported_at")
	}
	if ma.EventCount != 6 || ma.BlobCount != 1 {
		t.Fatalf("manifest counts: %+v", ma)
	}
	same := []string{"events.ndjson", "README.md"}
	for _, name := range same {
		aa, err := os.ReadFile(filepath.Join(a, name))
		if err != nil {
			t.Fatal(err)
		}
		bb, err := os.ReadFile(filepath.Join(b, name))
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Equal(aa, bb) {
			t.Fatalf("%s differs between exports", name)
		}
	}
	blobA, err := os.ReadFile(filepath.Join(a, "blobs", "sha256", helloSHA[:2], helloSHA[2:4], helloSHA))
	if err != nil {
		t.Fatal(err)
	}
	blobB, err := os.ReadFile(filepath.Join(b, "blobs", "sha256", helloSHA[:2], helloSHA[2:4], helloSHA))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(blobA, blobB) {
		t.Fatal("blob bytes differ")
	}

	rawA, err := os.ReadFile(filepath.Join(a, "manifest.json"))
	if err != nil {
		t.Fatal(err)
	}
	rawB, err := os.ReadFile(filepath.Join(b, "manifest.json"))
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Equal(rawA, rawB) && ma.ExportedAt != mb.ExportedAt {
		t.Fatal("manifest identical despite different timestamps")
	}
	var ja, jb map[string]any
	if err := json.Unmarshal(rawA, &ja); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(rawB, &jb); err != nil {
		t.Fatal(err)
	}
	delete(ja, "exported_at")
	delete(jb, "exported_at")
	ea, _ := json.Marshal(ja)
	eb, _ := json.Marshal(jb)
	if !bytes.Equal(ea, eb) {
		t.Fatalf("manifest differs beyond timestamp:\n%s\n%s", ea, eb)
	}
}

func TestExportManifestMatchesContents(t *testing.T) {
	root := filepath.Join("..", "..", "testdata", "m0")
	out := t.TempDir()
	man, err := Export(root, out)
	if err != nil {
		t.Fatal(err)
	}
	if man.ToolVersion != version.Version || man.ArchiveVersion != ArchiveVersion {
		t.Fatalf("manifest meta: %+v", man)
	}
	raw, err := os.ReadFile(filepath.Join(out, "events.ndjson"))
	if err != nil {
		t.Fatal(err)
	}
	n := 0
	for _, line := range bytes.Split(raw, []byte("\n")) {
		if len(bytes.TrimSpace(line)) > 0 {
			n++
		}
	}
	if n != man.EventCount {
		t.Fatalf("event_count=%d lines=%d", man.EventCount, n)
	}
	listed, err := blobs.List(out)
	if err != nil {
		t.Fatal(err)
	}
	if len(listed) != man.BlobCount || len(man.Blobs) != man.BlobCount {
		t.Fatalf("blob_count=%d listed=%d man.Blobs=%d", man.BlobCount, len(listed), len(man.Blobs))
	}
	for i, b := range man.Blobs {
		if b.SHA256 != listed[i].SHA256 {
			t.Fatalf("blob[%d] sha %s != %s", i, b.SHA256, listed[i].SHA256)
		}
		got, size, err := blobs.HashFile(listed[i].Path)
		if err != nil {
			t.Fatal(err)
		}
		if got != b.SHA256 || size != b.Size {
			t.Fatalf("hash/size mismatch %s size=%d want %s %d", got, size, b.SHA256, b.Size)
		}
	}
}

func TestExportRejectsCorruptBlob(t *testing.T) {
	root := copyM0(t)
	p, err := blobs.Path(root, helloSHA)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(p, []byte("this is not the hello fixture\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	out := t.TempDir()
	_, err = Export(root, out)
	if err == nil || !strings.Contains(err.Error(), "mismatch") {
		t.Fatalf("expected mismatch, got %v", err)
	}
	if _, err := os.Stat(filepath.Join(out, "blobs", "sha256", helloSHA[:2], helloSHA[2:4], helloSHA)); !os.IsNotExist(err) {
		t.Fatal("corrupt blob was copied")
	}
}

func TestFsckCleanFixture(t *testing.T) {
	rep, err := Fsck(filepath.Join("..", "..", "testdata", "m0"))
	if err != nil {
		t.Fatal(err)
	}
	if !rep.OK() {
		t.Fatalf("issues: %+v", rep.Issues)
	}
	if rep.Events != 6 || rep.Blobs != 1 {
		t.Fatalf("counts: %+v", rep)
	}
}

func TestFsckCorruptMissingOrphan(t *testing.T) {
	t.Run("corrupt", func(t *testing.T) {
		root := copyM0(t)
		p, err := blobs.Path(root, helloSHA)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(p, []byte("bit rot"), 0o644); err != nil {
			t.Fatal(err)
		}
		rep, err := Fsck(root)
		if err != nil {
			t.Fatal(err)
		}
		if n := countKind(rep, "mismatch"); n != 1 {
			t.Fatalf("mismatch=%d issues=%+v", n, rep.Issues)
		}
	})
	t.Run("missing", func(t *testing.T) {
		root := copyM0(t)
		p, err := blobs.Path(root, helloSHA)
		if err != nil {
			t.Fatal(err)
		}
		if err := os.Remove(p); err != nil {
			t.Fatal(err)
		}
		rep, err := Fsck(root)
		if err != nil {
			t.Fatal(err)
		}
		if n := countKind(rep, "missing"); n != 1 {
			t.Fatalf("missing=%d issues=%+v", n, rep.Issues)
		}
	})
	t.Run("orphan", func(t *testing.T) {
		root := copyM0(t)
		hash, _, _, err := blobs.Put(root, strings.NewReader("orphan blob body\n"))
		if err != nil {
			t.Fatal(err)
		}
		rep, err := Fsck(root)
		if err != nil {
			t.Fatal(err)
		}
		if n := countKind(rep, "orphan"); n != 1 {
			t.Fatalf("orphan=%d issues=%+v", n, rep.Issues)
		}
		if rep.Issues[len(rep.Issues)-1].SHA256 != hash && !hasOrphan(rep, hash) {
			t.Fatalf("expected orphan %s in %+v", hash, rep.Issues)
		}
	})
}

func TestRoundTripCounts(t *testing.T) {
	src := copyM0(t)
	if err := log.Append(src, event.Event{
		ID:   "01900000-0000-7000-8000-000000000010",
		TS:   "2024-01-15T12:06:00.000000000Z",
		Dev:  "manual",
		Type: event.TypeNote,
		Body: json.RawMessage(`{"text":"round-trip"}`),
	}); err != nil {
		t.Fatal(err)
	}
	if err := projection.Rebuild(src); err != nil {
		t.Fatal(err)
	}
	db, err := projection.Open(src)
	if err != nil {
		t.Fatal(err)
	}
	want, err := projection.TableCounts(db)
	db.Close()
	if err != nil {
		t.Fatal(err)
	}

	out := t.TempDir()
	if _, err := Export(src, out); err != nil {
		t.Fatal(err)
	}
	dest := t.TempDir()
	if err := Restore(out, dest); err != nil {
		t.Fatal(err)
	}
	if err := projection.Rebuild(dest); err != nil {
		t.Fatal(err)
	}
	db2, err := projection.Open(dest)
	if err != nil {
		t.Fatal(err)
	}
	got, err := projection.TableCounts(db2)
	db2.Close()
	if err != nil {
		t.Fatal(err)
	}
	if got != want {
		t.Fatalf("counts src=%+v dest=%+v", want, got)
	}
	evs1, err := log.ReadAll(src)
	if err != nil {
		t.Fatal(err)
	}
	evs2, err := log.ReadAll(dest)
	if err != nil {
		t.Fatal(err)
	}
	if len(evs1) != len(evs2) {
		t.Fatalf("events %d vs %d", len(evs1), len(evs2))
	}
	for i := range evs1 {
		if evs1[i].ID != evs2[i].ID || evs1[i].Type != evs2[i].Type {
			t.Fatalf("event %d: %+v vs %+v", i, evs1[i], evs2[i])
		}
	}
}

func TestRestoreIdempotent(t *testing.T) {
	src := filepath.Join("..", "..", "testdata", "m0")
	out := t.TempDir()
	if _, err := Export(src, out); err != nil {
		t.Fatal(err)
	}
	dest := t.TempDir()
	if err := Restore(out, dest); err != nil {
		t.Fatal(err)
	}
	if err := Restore(out, dest); err != nil {
		t.Fatal(err)
	}
	if err := projection.Rebuild(dest); err != nil {
		t.Fatal(err)
	}
	db, err := projection.Open(dest)
	if err != nil {
		t.Fatal(err)
	}
	got, err := projection.TableCounts(db)
	db.Close()
	if err != nil {
		t.Fatal(err)
	}
	if got.Events != 6 || got.Blobs != 1 {
		t.Fatalf("after double restore: %+v", got)
	}
	evs, err := log.ReadAll(dest)
	if err != nil {
		t.Fatal(err)
	}
	if len(evs) != 6 {
		t.Fatalf("events after double restore=%d", len(evs))
	}
}

func TestFsckTornTail(t *testing.T) {
	root := copyM0(t)
	path := filepath.Join(root, "log", "manual", "2024-01.ndjson")
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, append(mustRead(t, path), []byte(`{"id":"torn`)...), 0o600); err != nil {
		t.Fatal(err)
	}
	rep, err := Fsck(root)
	if err != nil {
		t.Fatal(err)
	}
	if len(rep.Diagnostics) != 1 || rep.Diagnostics[0].Kind != "torn_tail" {
		t.Fatalf("diagnostics=%+v", rep.Diagnostics)
	}
	if !rep.OK() {
		t.Fatalf("torn tail is diagnostic, not corruption: %+v", rep.Issues)
	}
	if rep.Events != 6 || rep.Blobs != 1 {
		t.Fatalf("counts after torn tail: %+v", rep)
	}
	var buf bytes.Buffer
	rep.Write(&buf)
	if !strings.Contains(buf.String(), "torn tail") || !strings.Contains(buf.String(), path) {
		t.Fatalf("report: %s", buf.String())
	}
	if !strings.Contains(buf.String(), "offset=") {
		t.Fatalf("report missing offset: %s", buf.String())
	}
	if !strings.Contains(buf.String(), strconv.FormatInt(info.Size(), 10)) {
		t.Fatalf("report missing expected offset %d: %s", info.Size(), buf.String())
	}
	if !strings.Contains(buf.String(), "ok 6 events 1 blobs") {
		t.Fatalf("report should remain healthy: %s", buf.String())
	}
	if got := mustRead(t, path); !bytes.Equal(got, append(mustRead(t, filepath.Join("..", "..", "testdata", "m0", "log", "manual", "2024-01.ndjson")), []byte(`{"id":"torn`)...)) {
		t.Fatal("fsck repaired or rewrote the torn log")
	}
}

func mustRead(t *testing.T, path string) []byte {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return b
}

func TestExportFileModes(t *testing.T) {
	root := filepath.Join("..", "..", "testdata", "m0")
	out := filepath.Join(t.TempDir(), "export")
	if _, err := Export(root, out); err != nil {
		t.Fatal(err)
	}
	for _, name := range []string{"events.ndjson", "README.md", "manifest.json"} {
		fi, err := os.Stat(filepath.Join(out, name))
		if err != nil {
			t.Fatal(err)
		}
		if fi.Mode().Perm() != 0o600 {
			t.Fatalf("%s mode %o", name, fi.Mode().Perm())
		}
	}
	di, err := os.Stat(out)
	if err != nil {
		t.Fatal(err)
	}
	if di.Mode().Perm() != 0o700 {
		t.Fatalf("export dir mode %o", di.Mode().Perm())
	}
}

func copyM0(t *testing.T) string {
	t.Helper()
	dst := t.TempDir()
	src := filepath.Join("..", "..", "testdata", "m0")
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
			return os.MkdirAll(target, 0o700)
		}
		b, err := os.ReadFile(path)
		if err != nil {
			return err
		}
		if err := os.MkdirAll(filepath.Dir(target), 0o755); err != nil {
			return err
		}
		return os.WriteFile(target, b, 0o644)
	})
	if err != nil {
		t.Fatal(err)
	}
	return dst
}

func countKind(r Report, kind string) int {
	n := 0
	for _, is := range r.Issues {
		if is.Kind == kind {
			n++
		}
	}
	return n
}

func hasOrphan(r Report, hash string) bool {
	for _, is := range r.Issues {
		if is.Kind == "orphan" && is.SHA256 == hash {
			return true
		}
	}
	return false
}
