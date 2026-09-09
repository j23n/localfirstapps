package main

import (
	"bytes"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"archive/internal/blobs"
	"archive/internal/log"
	"archive/internal/portable"
	"archive/internal/projection"
)

func TestExportAndFsckCLI(t *testing.T) {
	root := t.TempDir()
	hello := filepath.Join("..", "..", "testdata", "blobs", "hello.txt")
	runOK(t, "-root", root, "append", "-dev", "manual", "-type", "note", "-body", `{"text":"m2"}`)
	runOK(t, "-root", root, "import", "-dev", "garmin", hello)
	z := zipFixture(t)
	runOK(t, "-root", root, "import", "-dev", "apple", z)
	runOK(t, "-root", root, "rebuild")

	db, err := projection.Open(root)
	if err != nil {
		t.Fatal(err)
	}
	want, err := projection.TableCounts(db)
	db.Close()
	if err != nil {
		t.Fatal(err)
	}

	fsck := runOK(t, "-root", root, "fsck")
	if !strings.Contains(fsck, "ok") {
		t.Fatalf("fsck: %s", fsck)
	}

	out1 := filepath.Join(t.TempDir(), "e1")
	out2 := filepath.Join(t.TempDir(), "e2")
	runOK(t, "-root", root, "export", "-out", out1)
	runOK(t, "-root", root, "export", "-out", out2)

	events1, err := os.ReadFile(filepath.Join(out1, "events.ndjson"))
	if err != nil {
		t.Fatal(err)
	}
	events2, err := os.ReadFile(filepath.Join(out2, "events.ndjson"))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(events1, events2) {
		t.Fatal("events.ndjson not deterministic")
	}
	readme1, _ := os.ReadFile(filepath.Join(out1, "README.md"))
	readme2, _ := os.ReadFile(filepath.Join(out2, "README.md"))
	if !bytes.Equal(readme1, readme2) {
		t.Fatal("README.md not deterministic")
	}

	var m1, m2 portable.Manifest
	b1, _ := os.ReadFile(filepath.Join(out1, "manifest.json"))
	b2, _ := os.ReadFile(filepath.Join(out2, "manifest.json"))
	if err := json.Unmarshal(b1, &m1); err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(b2, &m2); err != nil {
		t.Fatal(err)
	}
	if m1.EventCount != want.Events || m1.BlobCount != want.Blobs {
		t.Fatalf("manifest %+v vs counts %+v", m1, want)
	}
	m1.ExportedAt = ""
	m2.ExportedAt = ""
	if m1.EventCount != m2.EventCount || m1.BlobCount != m2.BlobCount {
		t.Fatal("manifest counts differ")
	}

	dest := t.TempDir()
	if err := portable.Restore(out1, dest); err != nil {
		t.Fatal(err)
	}
	runOK(t, "-root", dest, "rebuild")
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
		t.Fatalf("round-trip counts want=%+v got=%+v", want, got)
	}
	n1, err := log.ReadAll(root)
	if err != nil {
		t.Fatal(err)
	}
	n2, err := log.ReadAll(dest)
	if err != nil {
		t.Fatal(err)
	}
	if len(n1) != len(n2) {
		t.Fatalf("events %d vs %d", len(n1), len(n2))
	}
}

func TestExportCorruptBlobCLI(t *testing.T) {
	root := t.TempDir()
	hello := filepath.Join("..", "..", "testdata", "blobs", "hello.txt")
	runOK(t, "-root", root, "import", "-dev", "garmin", hello)
	p, err := blobs.Path(root, "a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(p, []byte("rot"), 0o644); err != nil {
		t.Fatal(err)
	}
	var out, errb bytes.Buffer
	code := run(&out, &errb, []string{"-root", root, "export", "-out", t.TempDir()})
	if code == 0 || !strings.Contains(errb.String(), "mismatch") {
		t.Fatalf("code=%d out=%s err=%s", code, out.String(), errb.String())
	}
}

func TestFsckDetectsProblemsCLI(t *testing.T) {
	root := t.TempDir()
	hello := filepath.Join("..", "..", "testdata", "blobs", "hello.txt")
	runOK(t, "-root", root, "import", "-dev", "garmin", hello)

	p, err := blobs.Path(root, "a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(p, []byte("rot"), 0o644); err != nil {
		t.Fatal(err)
	}
	var out, errb bytes.Buffer
	if code := run(&out, &errb, []string{"-root", root, "fsck"}); code == 0 {
		t.Fatal("fsck should fail on corrupt blob")
	}
	if !strings.Contains(out.String(), "mismatch") {
		t.Fatalf("fsck: %s %s", out.String(), errb.String())
	}
}

func TestCloudSyncWarning(t *testing.T) {
	root := filepath.Join(t.TempDir(), "Dropbox", "archive")
	if err := os.MkdirAll(root, 0o755); err != nil {
		t.Fatal(err)
	}
	var out, errb bytes.Buffer
	code := run(&out, &errb, []string{"-root", root, "append", "-dev", "manual", "-type", "note", "-body", `{"text":"x"}`})
	if code != 0 {
		t.Fatalf("code=%d err=%s", code, errb.String())
	}
	if !strings.Contains(errb.String(), "warning:") || !strings.Contains(strings.ToLower(errb.String()), "dropbox") {
		t.Fatalf("expected cloud warning, got %q", errb.String())
	}
}

func TestGitignoreCoversDataDirs(t *testing.T) {
	b, err := os.ReadFile(filepath.Join("..", "..", ".gitignore"))
	if err != nil {
		t.Fatal(err)
	}
	s := string(b)
	for _, line := range []string{"/archive/", "/data/", "/export/", "/exports/"} {
		if !strings.Contains(s, line) {
			t.Fatalf("gitignore missing %s", line)
		}
	}
}
