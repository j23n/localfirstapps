package main

import (
	"archive/zip"
	"io"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestAppleImportOneEvent(t *testing.T) {
	root := t.TempDir()
	z := zipFixture(t)
	out := runOK(t, "-root", root, "import", "-dev", "apple", z)
	if !strings.Contains(out, `"type":"blob_import"`) || !strings.Contains(out, `"kind":"apple"`) {
		t.Fatalf("import: %s", out)
	}
	if strings.Count(out, "blob_import") != 1 {
		t.Fatalf("expected one event: %s", out)
	}
	again := runOK(t, "-root", root, "import", "-dev", "apple", z)
	if !strings.Contains(again, "idempotent") {
		t.Fatalf("re-import: %s", again)
	}
	runOK(t, "-root", root, "rebuild")
	obs := runOK(t, "-root", root, "query", "observations", "-kind", "HKQuantityTypeIdentifierHeartRate")
	if !strings.Contains(obs, `"kind":"HKQuantityTypeIdentifierHeartRate"`) {
		t.Fatalf("query observations: %s", obs[:min(200, len(obs))])
	}
}

func TestImportExtensionlessExportXML(t *testing.T) {
	root := t.TempDir()
	src := filepath.Join("..", "..", "testdata", "apple", "export.xml")
	bare := filepath.Join(t.TempDir(), "exportblob")
	in, err := os.Open(src)
	if err != nil {
		t.Fatal(err)
	}
	out, err := os.Create(bare)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := io.Copy(out, in); err != nil {
		t.Fatal(err)
	}
	in.Close()
	if err := out.Close(); err != nil {
		t.Fatal(err)
	}

	imp := runOK(t, "-root", root, "import", "-dev", "apple", bare)
	if !strings.Contains(imp, `"type":"blob_import"`) {
		t.Fatalf("import: %s", imp)
	}
	runOK(t, "-root", root, "rebuild")
	obs := runOK(t, "-root", root, "query", "observations")
	n := 0
	for _, line := range strings.Split(obs, "\n") {
		if strings.TrimSpace(line) != "" {
			n++
		}
	}
	if n == 0 {
		t.Fatal("extension-less export.xml produced zero observations")
	}
}

func zipFixture(t *testing.T) string {
	t.Helper()
	dest := filepath.Join(t.TempDir(), "export.zip")
	f, err := os.Create(dest)
	if err != nil {
		t.Fatal(err)
	}
	zw := zip.NewWriter(f)
	w, err := zw.Create("apple_health_export/export.xml")
	if err != nil {
		t.Fatal(err)
	}
	src, err := os.Open(filepath.Join("..", "..", "testdata", "apple", "export.xml"))
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
	return dest
}

func min(a, b int) int {
	if a < b {
		return a
	}
	return b
}
