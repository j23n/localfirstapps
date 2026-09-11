package apple

import (
	"archive/zip"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestParseGPXFixture(t *testing.T) {
	f, err := os.Open(filepath.Join("..", "..", "..", "testdata", "apple", "workout-routes", "route_2025-09-14_8.17pm.gpx"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	pts, err := ParseGPX(f)
	if err != nil {
		t.Fatal(err)
	}
	if len(pts) != 2526 {
		t.Fatalf("points=%d want 2526", len(pts))
	}
	// Fixture coordinates are offset from the real route; only check that
	// lat/lon were read into the right fields and are plausible.
	if p := pts[0]; p.Lat == 0 || p.Lon == 0 || p.Lat < -90 || p.Lat > 90 || p.Lon < -180 || p.Lon > 180 {
		t.Fatalf("first %+v", pts[0])
	}
	if pts[0].Ele == nil || pts[len(pts)-1].Ele == nil {
		t.Fatal("fixture points have <ele>")
	}
	if pts[0].Time.IsZero() || pts[0].Time.Location() != nil && pts[0].Time.UTC().Hour() != 17 {
		// 17:35:34Z
	}
	if pts[0].Time.UTC().Format("15:04:05") != "17:35:34" {
		t.Fatalf("first time %s", pts[0].Time)
	}
}

func TestParseGPXMissingEleIsNull(t *testing.T) {
	// Format contract: absent <ele> must not become 0.
	const src = `<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" xmlns="http://www.topografix.com/GPX/1/1">
  <trk><trkseg>
    <trkpt lat="42.68" lon="23.28"><time>2025-09-14T17:35:34Z</time></trkpt>
    <trkpt lat="42.69" lon="23.29"><ele>591.2</ele><time>2025-09-14T17:35:35Z</time></trkpt>
  </trkseg></trk>
</gpx>`
	pts, err := ParseGPX(strings.NewReader(src))
	if err != nil {
		t.Fatal(err)
	}
	if len(pts) != 2 {
		t.Fatalf("n=%d", len(pts))
	}
	if pts[0].Ele != nil {
		t.Fatalf("missing ele became %v", *pts[0].Ele)
	}
	if pts[1].Ele == nil || *pts[1].Ele != 591.2 {
		t.Fatalf("present ele=%v", pts[1].Ele)
	}
}

func TestParseGPXHugeCharDataCapped(t *testing.T) {
	huge := strings.Repeat("x", 1<<20)
	src := `<?xml version="1.0" encoding="UTF-8"?>
<gpx version="1.1" xmlns="http://www.topografix.com/GPX/1/1">
  <trk><trkseg>
    <trkpt lat="42.68" lon="23.28"><time>` + huge + `</time></trkpt>
  </trkseg></trk>
</gpx>`
	pts, err := ParseGPX(strings.NewReader(src))
	if err != nil {
		t.Fatal(err)
	}
	if len(pts) != 1 {
		t.Fatalf("n=%d", len(pts))
	}
	if pts[0].Lat != 42.68 || pts[0].Lon != 23.28 {
		t.Fatalf("pt=%+v", pts[0])
	}
	if !pts[0].Time.IsZero() {
		t.Fatal("huge <time> must not parse as a timestamp")
	}
}

func TestOpenMemberFileReference(t *testing.T) {
	dir := t.TempDir()
	zpath := filepath.Join(dir, "export.zip")
	gpx := filepath.Join("..", "..", "..", "testdata", "apple", "workout-routes", "route_2025-09-14_8.17pm.gpx")
	zf, err := os.Create(zpath)
	if err != nil {
		t.Fatal(err)
	}
	zw := zip.NewWriter(zf)
	w, err := zw.Create("apple_health_export/workout-routes/route_2025-09-14_8.17pm.gpx")
	if err != nil {
		t.Fatal(err)
	}
	src, err := os.ReadFile(gpx)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := w.Write(src); err != nil {
		t.Fatal(err)
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	zf.Close()

	rc, err := OpenMember(zpath, "/workout-routes/route_2025-09-14_8.17pm.gpx")
	if err != nil {
		t.Fatal(err)
	}
	defer rc.Close()
	pts, err := ParseGPX(rc)
	if err != nil {
		t.Fatal(err)
	}
	if len(pts) != 2526 {
		t.Fatalf("opened member points=%d", len(pts))
	}
}
