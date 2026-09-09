package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func runOK(t *testing.T, args ...string) string {
	t.Helper()
	var out, errb bytes.Buffer
	if code := run(&out, &errb, args); code != 0 {
		t.Fatalf("run %v: code=%d err=%s", args, code, errb.String())
	}
	return out.String()
}

func TestM0ExitCriteria(t *testing.T) {
	root := t.TempDir()
	hello := filepath.Join("..", "..", "testdata", "blobs", "hello.txt")

	line := runOK(t, "-root", root, "append", "-dev", "manual", "-type", "note", "-body", `{"text":"m0"}`)
	var ev struct {
		ID   string          `json:"id"`
		TS   string          `json:"ts"`
		Dev  string          `json:"dev"`
		Type string          `json:"type"`
		Body json.RawMessage `json:"body"`
	}
	if err := json.Unmarshal([]byte(strings.TrimSpace(line)), &ev); err != nil {
		t.Fatalf("append output: %v\n%s", err, line)
	}
	if ev.Dev != "manual" || ev.Type != "note" || !strings.HasSuffix(ev.TS, "Z") {
		t.Fatalf("event: %+v", ev)
	}
	month := ev.TS[:7]
	onDisk, err := os.ReadFile(filepath.Join(root, "log", "manual", month+".ndjson"))
	if err != nil {
		t.Fatal(err)
	}
	if string(onDisk) != line {
		t.Fatalf("log line mismatch:\n%s\n%s", onDisk, line)
	}

	imp := runOK(t, "-root", root, "import", "-dev", "garmin", hello)
	if !strings.Contains(imp, `"type":"blob_import"`) {
		t.Fatalf("import: %s", imp)
	}
	again := runOK(t, "-root", root, "import", "-dev", "garmin", hello)
	if !strings.Contains(again, "idempotent") {
		t.Fatalf("expected idempotent re-import, got %s", again)
	}
	entries, err := os.ReadDir(filepath.Join(root, "log", "garmin"))
	if err != nil {
		t.Fatal(err)
	}
	var nlines int
	for _, e := range entries {
		b, err := os.ReadFile(filepath.Join(root, "log", "garmin", e.Name()))
		if err != nil {
			t.Fatal(err)
		}
		nlines += strings.Count(string(b), "\n")
	}
	if nlines != 1 {
		t.Fatalf("garmin log lines=%d", nlines)
	}

	db1 := strings.TrimSpace(runOK(t, "-root", root, "rebuild"))
	q := runOK(t, "-root", root, "query", "-type", "note")
	if !strings.Contains(q, `"text":"m0"`) {
		t.Fatalf("query missed append: %s", q)
	}
	if !strings.Contains(runOK(t, "-root", root, "query", "-type", "blob_import"), "a70940623490fa4c251737cf74e1bf75a0327bb18766cc5620edbde3a985c96d") {
		t.Fatal("query missed blob_import")
	}

	sum := func(path string) string {
		b, err := os.ReadFile(path)
		if err != nil {
			t.Fatal(err)
		}
		h := sha256.Sum256(b)
		return hex.EncodeToString(h[:])
	}
	a := sum(db1)
	db2 := strings.TrimSpace(runOK(t, "-root", root, "rebuild"))
	if db1 != db2 {
		t.Fatalf("rebuild path changed %s %s", db1, db2)
	}
	if sum(db2) != a {
		t.Fatal("consecutive rebuilds are not byte-identical")
	}
}
