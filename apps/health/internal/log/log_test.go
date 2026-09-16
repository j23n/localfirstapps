package log

import (
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"archive/internal/event"
)

func TestAppendWritesLine(t *testing.T) {
	root := t.TempDir()
	ev := event.Event{
		ID:   "01900000-0000-7000-8000-0000000000aa",
		TS:   "2024-06-02T08:00:00.000000000Z",
		Dev:  "manual",
		Type: event.TypeNote,
		Body: json.RawMessage(`{"text":"hello"}`),
	}
	if err := Append(root, ev); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(root, "log", "manual", "2024-06.ndjson")
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	want := `{"id":"01900000-0000-7000-8000-0000000000aa","ts":"2024-06-02T08:00:00.000000000Z","dev":"manual","type":"note","body":{"text":"hello"}}` + "\n"
	if string(b) != want {
		t.Fatalf("got %s", b)
	}
	// second append is another line, never a rewrite
	ev.ID = "01900000-0000-7000-8000-0000000000ab"
	ev.TS = "2024-06-02T09:00:00.000000000Z"
	if err := Append(root, ev); err != nil {
		t.Fatal(err)
	}
	b, err = os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if got := string(b); len(got) < 2*len(want) || got[:len(want)] != want {
		t.Fatalf("append-only violated:\n%s", got)
	}
}

func TestReadAllFixture(t *testing.T) {
	root := filepath.Join("..", "..", "testdata", "m0")
	evs, err := ReadAll(root)
	if err != nil {
		t.Fatal(err)
	}
	if len(evs) != 6 {
		t.Fatalf("len=%d", len(evs))
	}
	if evs[0].ID != "01900000-0000-7000-8000-000000000001" || evs[0].Type != event.TypeNote {
		t.Fatalf("first: %+v", evs[0])
	}
	ok, err := HasBlobImport(root, "a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d")
	if err != nil {
		t.Fatal(err)
	}
	if !ok {
		t.Fatal("missing blob_import in fixture")
	}
}

func TestReadReportTornTail(t *testing.T) {
	dst := t.TempDir()
	src := filepath.Join("..", "..", "testdata", "m0", "log")
	if err := copyLogTree(dst, src); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dst, "log", "manual", "2024-01.ndjson")
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	offset := info.Size()
	f, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0o600)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := f.Write([]byte(`{"id":"01900000-torn`)); err != nil {
		t.Fatal(err)
	}
	if err := f.Close(); err != nil {
		t.Fatal(err)
	}

	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	rep, err := ReadReport(dst)
	if err != nil {
		t.Fatal(err)
	}
	if len(rep.Events) != 6 {
		t.Fatalf("complete events=%d want 6", len(rep.Events))
	}
	if len(rep.TornTails) != 1 {
		t.Fatalf("torn tails=%d want 1", len(rep.TornTails))
	}
	diag := &rep.TornTails[0]
	if diag.Path != path {
		t.Fatalf("path=%s want %s", diag.Path, path)
	}
	if diag.Offset != offset {
		t.Fatalf("offset=%d want %d", diag.Offset, offset)
	}
	if diag.Err == nil {
		t.Fatal("missing wrapped JSON error")
	}
	after, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(after) != string(before) {
		t.Fatal("ReadReport repaired or rewrote the log")
	}

	evs, err := ReadAll(dst)
	var torn *TornTailError
	if !errors.As(err, &torn) {
		t.Fatalf("want TornTailError, got %v", err)
	}
	if len(evs) != 6 {
		t.Fatalf("ReadAll complete events=%d want 6", len(evs))
	}
}

func TestReadReportMidFileMalformedIsHardError(t *testing.T) {
	root := t.TempDir()
	dir := filepath.Join(root, "log", "manual")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "2024-01.ndjson")
	body := `{"id":"a","ts":"2024-01-15T12:00:00.000000000Z","dev":"manual","type":"note","body":{"text":"ok"}}` + "\n" +
		"{not-json}\n" +
		`{"id":"b","ts":"2024-01-15T12:01:00.000000000Z","dev":"manual","type":"note","body":{"text":"ok"}}` + "\n"
	if err := os.WriteFile(path, []byte(body), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := ReadReport(root); err == nil || !strings.Contains(err.Error(), path+":2:") {
		t.Fatalf("want line 2 hard error, got %v", err)
	}
}

func TestAppendFileMode(t *testing.T) {
	root := t.TempDir()
	ev := event.Event{
		ID:   "01900000-0000-7000-8000-0000000000aa",
		TS:   "2024-06-02T08:00:00.000000000Z",
		Dev:  "manual",
		Type: event.TypeNote,
		Body: json.RawMessage(`{"text":"hello"}`),
	}
	if err := Append(root, ev); err != nil {
		t.Fatal(err)
	}
	dir := filepath.Join(root, "log", "manual")
	di, err := os.Stat(dir)
	if err != nil {
		t.Fatal(err)
	}
	if di.Mode().Perm() != 0o700 {
		t.Fatalf("log dir mode %o", di.Mode().Perm())
	}
	fi, err := os.Stat(filepath.Join(dir, "2024-06.ndjson"))
	if err != nil {
		t.Fatal(err)
	}
	if fi.Mode().Perm() != 0o600 {
		t.Fatalf("log file mode %o", fi.Mode().Perm())
	}
}

func copyLogTree(dst, src string) error {
	return filepath.Walk(src, func(path string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		rel, err := filepath.Rel(filepath.Dir(src), path)
		if err != nil {
			return err
		}
		target := filepath.Join(dst, rel)
		if info.IsDir() {
			return os.MkdirAll(target, 0o700)
		}
		in, err := os.Open(path)
		if err != nil {
			return err
		}
		defer in.Close()
		if err := os.MkdirAll(filepath.Dir(target), 0o700); err != nil {
			return err
		}
		out, err := os.OpenFile(target, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, 0o600)
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
}
