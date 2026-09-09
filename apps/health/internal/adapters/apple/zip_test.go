package apple

import (
	"archive/zip"
	"io"
	"os"
	"path/filepath"
	"testing"
)

func TestIsExportZipAndXML(t *testing.T) {
	xmlPath := fixture("export.xml")
	if !IsExport(xmlPath) {
		t.Fatal("export.xml not recognized")
	}
	zpath := writeZip(t, xmlPath, "apple_health_export/export.xml")
	if !IsExport(zpath) {
		t.Fatal("zip not recognized")
	}
	rc, err := OpenXML(zpath)
	if err != nil {
		t.Fatal(err)
	}
	defer rc.Close()
	_, obs, _, err := Collect(rc)
	if err != nil {
		t.Fatal(err)
	}
	if len(obs) < 200 {
		t.Fatalf("obs=%d", len(obs))
	}
}

func TestIsExportExtensionlessXML(t *testing.T) {
	src := fixture("export.xml")
	dst := filepath.Join(t.TempDir(), "blobhash")
	b, err := os.ReadFile(src)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(dst, b, 0o600); err != nil {
		t.Fatal(err)
	}
	if !IsExport(dst) {
		t.Fatal("extension-less export.xml not recognized")
	}
	rc, err := OpenXML(dst)
	if err != nil {
		t.Fatal(err)
	}
	defer rc.Close()
	_, obs, _, err := Collect(rc)
	if err != nil {
		t.Fatal(err)
	}
	if len(obs) == 0 {
		t.Fatal("OpenXML on extension-less file yielded no observations")
	}
}

func TestOpenMemberRejectsBareSuffix(t *testing.T) {
	zpath := writeZip(t, fixture("export.xml"), "apple_health_export/export.xml")
	if _, err := OpenMember(zpath, "ort.xml"); err == nil {
		t.Fatal("bare suffix fallback matched ort.xml to export.xml")
	}
	rc, err := OpenMember(zpath, "apple_health_export/export.xml")
	if err != nil {
		t.Fatal(err)
	}
	rc.Close()
	rc, err = OpenMember(zpath, "export.xml")
	if err != nil {
		t.Fatal(err)
	}
	rc.Close()
}

func writeZip(t *testing.T, xmlPath, name string) string {
	t.Helper()
	out := filepath.Join(t.TempDir(), "export.zip")
	f, err := os.Create(out)
	if err != nil {
		t.Fatal(err)
	}
	zw := zip.NewWriter(f)
	w, err := zw.Create(name)
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
	return out
}
