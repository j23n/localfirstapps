package projection

import (
	"archive/zip"
	"encoding/json"
	"io"
	"os"
	"path/filepath"
	"testing"
	"time"

	"archive/config"
	"archive/internal/adapters/apple"
)

func TestRDPKnownInput(t *testing.T) {
	// Equirectangular metres at the equator: 1° lon = metresPerDeg.
	mk := func(lat, lon float64) apple.TrackPoint {
		return apple.TrackPoint{Lat: lat, Lon: lon}
	}
	// Four colinear points → endpoints only.
	line := []apple.TrackPoint{mk(0, 0), mk(0, 0.0003), mk(0, 0.0006), mk(0, 0.001)}
	if out := rdpSegment(line, 10); len(out) != 2 {
		t.Fatalf("colinear kept %d want 2", len(out))
	}
	// 20 m north of the chord midpoint must survive 10 m tolerance.
	bump := 20 / metresPerDeg
	with := []apple.TrackPoint{mk(0, 0), mk(bump, 0.0005), mk(0, 0.001)}
	out := rdpSegment(with, 10)
	if len(out) != 3 {
		t.Fatalf("20m vertex kept %d want 3", len(out))
	}
	// 5 m north is inside the tolerance.
	with[1].Lat = 5 / metresPerDeg
	if out := rdpSegment(with, 10); len(out) != 2 {
		t.Fatalf("5m vertex kept %d want 2", len(out))
	}
}

func TestSplitAtPauseAboveThreshold(t *testing.T) {
	f, err := os.Open(filepath.Join("..", "..", "testdata", "apple", "workout-routes", "route_2025-09-23_gap.gpx"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	pts, err := apple.ParseGPX(f)
	if err != nil {
		t.Fatal(err)
	}
	if len(pts) < 4 {
		t.Fatalf("trimmed gap fixture too short: %d", len(pts))
	}
	above := splitTrack(pts, 60*time.Second, nil)
	if len(above) != 2 {
		t.Fatalf("60s threshold: segments=%d want 2 (2919s GPS gap)", len(above))
	}
	below := splitTrack(pts, 4000*time.Second, nil)
	if len(below) != 1 {
		t.Fatalf("threshold above the gap must not split: %d", len(below))
	}
}

func TestSubThresholdPauseCountedNotSplit(t *testing.T) {
	start := time.Date(2025, 9, 14, 17, 35, 34, 0, time.UTC)
	end := time.Date(2025, 9, 14, 18, 17, 47, 0, time.UTC)
	evs := []projectedEvent{
		{Kind: "HKWorkoutEventTypePause", Time: time.Date(2025, 9, 14, 18, 14, 21, 0, time.UTC)},
		{Kind: "HKWorkoutEventTypeResume", Time: time.Date(2025, 9, 14, 18, 14, 25, 0, time.UTC)},
		{Kind: "HKWorkoutEventTypePause", Time: end},
	}
	elapsed, moving, paused, n, splits := timeFromWorkout(start, end, evs, 60*time.Second)
	if paused != 4 || n != 2 {
		t.Fatalf("paused=%d n=%d (0s end pause still counts)", paused, n)
	}
	if len(splits) != 0 {
		t.Fatalf("4s pause must not mark a split: %v", splits)
	}
	if moving+paused != elapsed {
		t.Fatalf("moving %d + paused %d != elapsed %d", moving, paused, elapsed)
	}
	if elapsed != 2533 {
		t.Fatalf("elapsed=%d want 2533", elapsed)
	}
}

func TestFixtureWorkoutTiming(t *testing.T) {
	f, err := os.Open(filepath.Join("..", "..", "testdata", "apple", "export.xml"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	_, _, eps, err := apple.Collect(f)
	if err != nil {
		t.Fatal(err)
	}
	var run *apple.Episode
	for i := range eps {
		if eps[i].Kind == "HKWorkoutActivityTypeRunning" {
			run = &eps[i]
			break
		}
	}
	if run == nil {
		t.Fatal("no running workout")
	}
	evs := projectWorkoutEvents(asStringMaps(run.Body["events"]))
	var kinds []string
	for _, e := range evs {
		kinds = append(kinds, e.Kind)
	}
	if !hasKind(kinds, "HKWorkoutEventTypePause") || !hasKind(kinds, "HKWorkoutEventTypeResume") || !hasKind(kinds, "HKWorkoutEventTypeSegment") {
		t.Fatalf("fixture events=%v", kinds)
	}
	if hasKind(kinds, "HKWorkoutEventTypeMotionPaused") || hasKind(kinds, "HKWorkoutEventTypeLap") {
		t.Fatalf("unexpected kinds in this fixture: %v", kinds)
	}
	rt := deriveRoute(nil, evs, parseEpisodeTime(run.Start), parseEpisodeTime(run.End))
	if rt.MovingS+rt.PausedS != rt.ElapsedS {
		t.Fatalf("moving+paused != elapsed: %+v", rt)
	}
	if rt.PauseN != 2 || rt.PausedS != 4 {
		t.Fatalf("pause_count=%d paused_s=%d (open end Pause closes at end, 0s)", rt.PauseN, rt.PausedS)
	}
	if rt.Polyline != nil {
		t.Fatal("no GPX loaded here")
	}
}

func TestPolylineRoundTrip(t *testing.T) {
	segs := [][]apple.TrackPoint{
		{{Lat: 42.68, Lon: 23.28}, {Lat: 42.69, Lon: 23.29}},
	}
	b, warns := encodePolyline(segs)
	if len(warns) != 0 {
		t.Fatalf("warnings %v", warns)
	}
	if string(b[:4]) != PolylineMagic {
		t.Fatal(b[:4])
	}
	got, err := DecodePolyline(b)
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 1 || len(got[0]) != 2 {
		t.Fatalf("%+v", got)
	}
}

func TestGPXGoldenSimplify(t *testing.T) {
	config.ResetForTest()
	f, err := os.Open(filepath.Join("..", "..", "testdata", "apple", "workout-routes", "route_2025-09-14_8.17pm.gpx"))
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	pts, err := apple.ParseGPX(f)
	if err != nil {
		t.Fatal(err)
	}
	rt := deriveRoute(pts, nil, pts[0].Time, pts[len(pts)-1].Time)
	got := map[string]any{
		"raw": rtRaw(pts), "segments": rt.Segments, "points": rt.Points,
		"bbox": rt.BBox, "elapsed": rt.ElapsedS, "moving": rt.MovingS, "pauses": rt.PauseN,
	}
	if rt.AscentM != nil {
		got["ascent"] = *rt.AscentM
		got["descent"] = *rt.DescentM
	}
	path := filepath.Join("..", "..", "testdata", "apple", "golden-route.json")
	if os.Getenv("UPDATE_GOLDEN") != "" {
		b, _ := json.MarshalIndent(got, "", "  ")
		if err := os.WriteFile(path, append(b, '\n'), 0o644); err != nil {
			t.Fatal(err)
		}
		return
	}
	want, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("%v (run UPDATE_GOLDEN=1)", err)
	}
	var w map[string]any
	if err := json.Unmarshal(want, &w); err != nil {
		t.Fatal(err)
	}
	if int(w["raw"].(float64)) != len(pts) || int(w["points"].(float64)) != rt.Points || int(w["segments"].(float64)) != rt.Segments {
		t.Fatalf("got raw=%d segs=%d pts=%d want %v", len(pts), rt.Segments, rt.Points, w)
	}
	if w["bbox"] != rt.BBox {
		t.Fatalf("bbox %q want %q", rt.BBox, w["bbox"])
	}
}

func rtRaw(pts []apple.TrackPoint) int { return len(pts) }

func TestRebuildWithRouteAndNoRoute(t *testing.T) {
	root := t.TempDir()
	z := filepath.Join(t.TempDir(), "export.zip")
	zipApple(t, z, filepath.Join("..", "..", "testdata", "apple", "export.xml"),
		filepath.Join("..", "..", "testdata", "apple", "workout-routes", "route_2025-09-14_8.17pm.gpx"))
	importFile(t, root, "apple", z)
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	db, err := Open(root)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	var segs, pts, pauses, elapsed, moving, evN, polyN int
	var bbox string
	err = db.QueryRow(`SELECT route_segments, route_points, route_bbox, length(route_polyline), pause_count, elapsed_seconds, moving_seconds
		FROM episodes WHERE kind = 'HKWorkoutActivityTypeRunning'`).Scan(&segs, &pts, &bbox, &polyN, &pauses, &elapsed, &moving)
	if err != nil {
		t.Fatal(err)
	}
	if polyN < 16 {
		t.Fatalf("polyline bytes=%d", polyN)
	}
	if segs < 1 || pts < 10 || bbox == "" {
		t.Fatalf("route segs=%d pts=%d bbox=%q", segs, pts, bbox)
	}
	if moving+4 != elapsed || pauses != 2 {
		t.Fatalf("moving=%d elapsed=%d pauses=%d", moving, elapsed, pauses)
	}
	if err := db.QueryRow(`SELECT COUNT(*) FROM workout_event WHERE episode_id = (SELECT dedup_key FROM episodes WHERE kind='HKWorkoutActivityTypeRunning')`).Scan(&evN); err != nil {
		t.Fatal(err)
	}
	if evN != 13 {
		t.Fatalf("workout_event=%d want 13", evN)
	}
	var nullRoutes int
	mustCount(t, db, `SELECT COUNT(*) FROM episodes WHERE kind='ActivitySummary' AND route_polyline IS NULL AND moving_seconds IS NULL`, &nullRoutes)
	if nullRoutes != 12 {
		t.Fatalf("summaries with null route/timing=%d", nullRoutes)
	}

	a := fileSHA(t, DBPath(root))
	if err := Rebuild(root); err != nil {
		t.Fatal(err)
	}
	b := fileSHA(t, DBPath(root))
	if a != b {
		t.Fatal("rebuilds with route data are not byte-identical")
	}

	// XML-only zip: route columns stay null, timing still fills from WorkoutEvent.
	root2 := t.TempDir()
	z2 := filepath.Join(t.TempDir(), "xmlonly.zip")
	zipXML(t, filepath.Join("..", "..", "testdata", "apple", "export.xml"), z2)
	importFile(t, root2, "apple", z2)
	if err := Rebuild(root2); err != nil {
		t.Fatal(err)
	}
	db2, err := Open(root2)
	if err != nil {
		t.Fatal(err)
	}
	defer db2.Close()
	var poly2 []byte
	var moving2 int
	if err := db2.QueryRow(`SELECT route_polyline, moving_seconds FROM episodes WHERE kind='HKWorkoutActivityTypeRunning'`).Scan(&poly2, &moving2); err != nil {
		t.Fatal(err)
	}
	if poly2 != nil {
		t.Fatal("no GPX in blob must leave route_polyline null")
	}
	if moving2 <= 0 {
		t.Fatal("timing still comes from WorkoutEvent")
	}
}

func zipApple(t *testing.T, dest, xmlPath, gpxPath string) {
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
	gw, err := zw.Create("apple_health_export/workout-routes/" + filepath.Base(gpxPath))
	if err != nil {
		t.Fatal(err)
	}
	g, err := os.Open(gpxPath)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := io.Copy(gw, g); err != nil {
		t.Fatal(err)
	}
	g.Close()
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	f.Close()
}

func hasKind(in []string, want string) bool {
	for _, s := range in {
		if s == want {
			return true
		}
	}
	return false
}

func TestTimeFromWorkoutSortsEvents(t *testing.T) {
	start := time.Date(2025, 1, 1, 10, 0, 0, 0, time.UTC)
	end := start.Add(10 * time.Minute)
	evs := []projectedEvent{
		{Kind: "HKWorkoutEventTypeResume", Time: start.Add(2 * time.Minute)},
		{Kind: "HKWorkoutEventTypePause", Time: start.Add(1 * time.Minute)},
	}
	_, _, paused, n, _ := timeFromWorkout(start, end, evs, time.Minute)
	if n != 1 || paused != 60 {
		t.Fatalf("paused=%d n=%d want 60s/1 after sort", paused, n)
	}
}

func TestTimeFromWorkoutIgnoresSecondPause(t *testing.T) {
	start := time.Date(2025, 1, 1, 10, 0, 0, 0, time.UTC)
	end := start.Add(10 * time.Minute)
	evs := []projectedEvent{
		{Kind: "HKWorkoutEventTypePause", Time: start.Add(1 * time.Minute)},
		{Kind: "HKWorkoutEventTypePause", Time: start.Add(2 * time.Minute)},
		{Kind: "HKWorkoutEventTypeResume", Time: start.Add(3 * time.Minute)},
	}
	_, _, paused, n, _ := timeFromWorkout(start, end, evs, time.Minute)
	if n != 1 || paused != 120 {
		t.Fatalf("paused=%d n=%d want 120s from first pause", paused, n)
	}
}

func TestTimeFromWorkoutOpenPauseClosesAtEnd(t *testing.T) {
	start := time.Date(2025, 1, 1, 10, 0, 0, 0, time.UTC)
	end := start.Add(10 * time.Minute)
	evs := []projectedEvent{
		{Kind: "HKWorkoutEventTypePause", Time: start.Add(8 * time.Minute)},
	}
	elapsed, moving, paused, n, splits := timeFromWorkout(start, end, evs, time.Minute)
	if n != 1 || paused != 120 {
		t.Fatalf("paused=%d n=%d", paused, n)
	}
	if len(splits) != 1 {
		t.Fatalf("2min open pause should split: %v", splits)
	}
	if moving+paused != elapsed {
		t.Fatalf("moving %d + paused %d != elapsed %d", moving, paused, elapsed)
	}
}

func TestElevationHysteresis(t *testing.T) {
	ele := func(v float64) *float64 { return &v }
	pts := []apple.TrackPoint{
		{Ele: ele(100)},
		{Ele: ele(101.5)},
		{Ele: ele(99.5)},
		{Ele: ele(104)},
		{Ele: nil},
		{Ele: ele(100)},
	}
	a, d := elevationChange(pts)
	if a == nil || d == nil || *a != 4 || *d != 4 {
		t.Fatalf("ascent=%v descent=%v", a, d)
	}
	none := []apple.TrackPoint{{Lat: 1}, {Lat: 2}}
	a, d = elevationChange(none)
	if a != nil || d != nil {
		t.Fatal("no ele must stay nil")
	}
}

func TestEncodePolylineChunksLongSegment(t *testing.T) {
	const n = 70000
	seg := make([]apple.TrackPoint, n)
	for i := range seg {
		seg[i] = apple.TrackPoint{Lat: 42, Lon: 23 + float64(i)*1e-6}
	}
	b, warns := encodePolyline([][]apple.TrackPoint{seg})
	if len(warns) != 0 {
		t.Fatalf("warnings %v", warns)
	}
	got, err := DecodePolyline(b)
	if err != nil {
		t.Fatal(err)
	}
	total := 0
	for _, s := range got {
		total += len(s)
	}
	if total != n {
		t.Fatalf("decoded %d want %d segs=%d", total, n, len(got))
	}
	if len(got) != 2 {
		t.Fatalf("chunks=%d want 2", len(got))
	}
}
