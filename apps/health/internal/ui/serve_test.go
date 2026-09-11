package ui

import (
	"archive/zip"
	"bytes"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"testing"

	"archive/internal/blobs"
	"archive/internal/event"
	"archive/internal/log"
	"archive/internal/projection"
	"archive/internal/uuid"
)

func TestCheckBind(t *testing.T) {
	if err := CheckBind("127.0.0.1:8080"); err != nil {
		t.Fatal(err)
	}
	for _, addr := range []string{"0.0.0.0:8080", "[::]:8080", "localhost:8080", "192.168.1.2:80"} {
		if err := CheckBind(addr); err == nil {
			t.Fatalf("should refuse %s", addr)
		}
	}
	if err := CheckBind(":8080"); err == nil {
		t.Fatal("empty host binds all interfaces; must refuse")
	}
}

func TestListenRefusesBeforeListen(t *testing.T) {
	err := Listen("0.0.0.0:1", http.HandlerFunc(func(http.ResponseWriter, *http.Request) {}))
	if err == nil || !strings.Contains(err.Error(), "refusing to bind") {
		t.Fatalf("got %v", err)
	}
}

func localReq(method, target string, body io.Reader) *http.Request {
	req := httptest.NewRequest(method, target, body)
	req.Host = "127.0.0.1"
	return req
}

func fixtureRoot(t *testing.T) string {
	t.Helper()
	t.Setenv("ARCHIVE_TZ", "+0200")
	root := t.TempDir()
	z := filepath.Join(t.TempDir(), "export.zip")
	zipXML(t, filepath.Join("..", "..", "testdata", "apple", "export.xml"), z)
	importFile(t, root, "apple", z)
	if err := projection.Rebuild(root); err != nil {
		t.Fatal(err)
	}
	return root
}

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
	gpx := filepath.Join(filepath.Dir(xmlPath), "workout-routes", "route_2025-09-14_8.17pm.gpx")
	if gf, err := os.Open(gpx); err == nil {
		w, err := zw.Create("apple_health_export/workout-routes/route_2025-09-14_8.17pm.gpx")
		if err != nil {
			gf.Close()
			t.Fatal(err)
		}
		if _, err := io.Copy(w, gf); err != nil {
			gf.Close()
			t.Fatal(err)
		}
		gf.Close()
	}
	if err := zw.Close(); err != nil {
		t.Fatal(err)
	}
	f.Close()
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
	id, err := uuid.NewV7()
	if err != nil {
		t.Fatal(err)
	}
	body := []byte(`{"sha256":"` + hash + `","size":` + itoa(size) + `,"name":"` + filepath.Base(path) + `","kind":"apple"}`)
	ev := event.Event{ID: id, TS: "2026-09-08T12:00:00.000000000Z", Dev: dev, Type: event.TypeBlobImport, Body: body}
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

func TestGoldenHTML(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	h := srv.Handler()
	pages := []struct {
		name string
		path string
	}{
		{"today", "/"},
		{"medicine", "/medicine"},
		{"lifestyle", "/lifestyle"},
		{"sports", "/sports"},
		{"all", "/all"},
		{"kind", "/sports/stepcount"},
		{"gaps", "/gaps"},
		{"blobs", "/blobs"},
	}
	for _, p := range pages {
		req := localReq(http.MethodGet, p.path, nil)
		rec := httptest.NewRecorder()
		h.ServeHTTP(rec, req)
		if rec.Code != 200 {
			t.Fatalf("%s: %d %s", p.name, rec.Code, rec.Body.String())
		}
		got := normalizeGolden(rec.Body.Bytes())
		path := filepath.Join("testdata", "golden", p.name+".html")
		if os.Getenv("UPDATE_GOLDEN") != "" {
			if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(path, got, 0o644); err != nil {
				t.Fatal(err)
			}
			continue
		}
		want, err := os.ReadFile(path)
		if err != nil {
			t.Fatalf("%s: %v (run UPDATE_GOLDEN=1)", p.name, err)
		}
		if !bytes.Equal(got, want) {
			t.Errorf("%s: golden mismatch (%d vs %d bytes)", p.name, len(got), len(want))
		}
	}
}

var importTS = regexp.MustCompile(`<time class="import-ts">[^<]*</time>`)
var nonceAttr = regexp.MustCompile(`nonce="[^"]*"`)

func normalizeGolden(b []byte) []byte {
	b = importTS.ReplaceAll(b, []byte(`<time class="import-ts">TS</time>`))
	b = nonceAttr.ReplaceAll(b, []byte(`nonce="NONCE"`))
	b = bytes.ReplaceAll(b, []byte("\r\n"), []byte("\n"))
	if len(b) > 0 && b[len(b)-1] != '\n' {
		b = append(b, '\n')
	}
	return b
}

func TestUsageLog(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	req := localReq(http.MethodGet, "/sports?kind=StepCount&range=7d", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != 200 {
		t.Fatalf("code %d", rec.Code)
	}
	b, err := os.ReadFile(UsageLogPath(root))
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(string(b), "/sports") {
		t.Fatalf("usage log: %s", b)
	}
}

func TestReadOnlyRejectsPOST(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	req := localReq(http.MethodPost, "/all", strings.NewReader("x"))
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusMethodNotAllowed {
		t.Fatalf("code %d", rec.Code)
	}
}

func TestTodayFavouriteWorkouts(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	req := localReq(http.MethodGet, "/?on=2025-09-14", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	body := rec.Body.String()
	if rec.Code != 200 {
		t.Fatalf("code %d %s", rec.Code, body)
	}
	fav := strings.Index(body, "<h3>Favourites</h3>")
	run := strings.Index(body, "Running")
	if fav < 0 || run < 0 || run < fav {
		t.Fatal("Running workout should sit under Favourites")
	}
	if strings.Contains(body, "<h3>Workouts</h3>") {
		t.Fatal("workouts should not be a separate section when favourited")
	}
	re := regexp.MustCompile(`href="(/sports/running/[0-9a-f]+)"`)
	m := re.FindStringSubmatch(body)
	if m == nil {
		t.Fatal("today session should link to /sports/running/<id>")
	}
}

func TestWorkoutSessionPages(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	h := srv.Handler()
	req := localReq(http.MethodGet, "/sports/running", nil)
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != 200 {
		t.Fatalf("running list: %d %s", rec.Code, rec.Body.String())
	}
	list := rec.Body.String()
	if !strings.Contains(list, "<h3>Sessions</h3>") {
		t.Fatal("running page should list sessions")
	}
	re := regexp.MustCompile(`href="(/sports/running/[0-9a-f]+)"`)
	m := re.FindStringSubmatch(list)
	if m == nil {
		t.Fatalf("session link missing:\n%s", list)
	}
	req = localReq(http.MethodGet, m[1], nil)
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	body := rec.Body.String()
	if rec.Code != 200 {
		t.Fatalf("session: %d %s", rec.Code, body)
	}
	if !strings.Contains(body, "Running") || !strings.Contains(body, "min") {
		t.Fatalf("session detail:\n%s", body)
	}
	if strings.Contains(body, "Decoding into a polyline has not happened") {
		t.Fatal("session page still shows the old route placeholder")
	}
	if !strings.Contains(body, "Moving time") {
		t.Fatal("session page should show moving time")
	}
	if !strings.Contains(body, "route-svg") && !strings.Contains(body, "Rebuild the projection") {
		t.Fatal("session page should draw the stored route")
	}
	if !strings.Contains(body, `class="chart-data"`) {
		t.Fatal("session traces should be Chart.js data, like the other graphs")
	}
	if !strings.Contains(body, `"xTitle"`) || !strings.Contains(body, `"yTitle"`) {
		t.Fatal("session charts need axis titles")
	}
	if strings.Contains(body, `href="`+m[1]+`"`) {
		t.Fatal("detail page should not wrap the session in a self-link")
	}
	req = localReq(http.MethodGet, "/sports/hiking", nil)
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != 404 && rec.Code != 200 {
		t.Fatalf("hiking: %d", rec.Code)
	}
}

func TestDayCardDoesNotShowStaleLastReading(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	// Latest fixture day is 2026-08-24. Watch StepCount last appears in 2025.
	req := localReq(http.MethodGet, "/?on=2026-08-24", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	body := rec.Body.String()
	if rec.Code != 200 {
		t.Fatalf("code %d", rec.Code)
	}
	i := strings.Index(body, `data-kind="HKQuantityTypeIdentifierStepCount"`)
	if i < 0 {
		t.Fatal("missing StepCount card")
	}
	chunk := body[i:]
	if j := strings.Index(chunk, `data-kind="`); j > 0 {
		// keep this card only — next data-kind starts the following card
		if k := strings.Index(chunk[len(`data-kind="HKQuantityTypeIdentifierStepCount"`):], `data-kind="`); k >= 0 {
			chunk = chunk[:len(`data-kind="HKQuantityTypeIdentifierStepCount"`)+k]
		}
	}
	if strings.Contains(chunk, "198") {
		t.Fatal("Watch's 2025 StepCount must not headline 2026-08-24")
	}
	if !strings.Contains(chunk, "232") && !strings.Contains(chunk, "464") {
		t.Fatal("phone-only 2026-08-24 must show iPhone steps, not a dash:\n" + chunk)
	}
	if !strings.Contains(chunk, "iPhone") {
		t.Fatal("phone-only day should name iPhone, not the unused Watch preference:\n" + chunk)
	}
}

func TestKindPageUnionsNonOverlappingSources(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	req := localReq(http.MethodGet, "/sports/stepcount?range=all", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	body := rec.Body.String()
	if rec.Code != 200 {
		t.Fatalf("code %d %s", rec.Code, body)
	}
	if !strings.Contains(body, "2026-08-24") {
		t.Fatal("phone-only day missing from the headline:\n" + body)
	}
	if !strings.Contains(body, "iPhone") || !strings.Contains(body, "Apple Watch") {
		t.Fatal("union series should keep both sources:\n" + body)
	}
	if !strings.Contains(body, "Sep 2025") || !strings.Contains(body, "Aug 2026") {
		t.Fatal("year view should keep both months, including empty ones between:\n" + body)
	}
	if !strings.Contains(body, "null") {
		t.Fatal("empty months must stay on the axis")
	}
	if strings.Contains(body, `name="source"`) {
		t.Fatal("picker must stay hidden when sources never share a local day")
	}
	if strings.Contains(body, "source=Apple") || strings.Contains(body, "source=Phone2") {
		t.Fatal("range links must not lock a source on a union kind")
	}
}

func TestKindPagePickerOnlyWhenSourcesOverlap(t *testing.T) {
	root := fixtureRoot(t)
	db, err := projection.Open(root)
	if err != nil {
		t.Fatal(err)
	}
	_, err = db.Exec(`INSERT INTO observations (dedup_key, n, kind, source, start_ts, end_ts, start_offset, metadata, value, unit)
		VALUES ('overlap-watch', 1, 'HKQuantityTypeIdentifierStepCount', 'Apple Watch',
			'2025-09-20T22:30:00.000000000Z', '2025-09-20T22:31:00.000000000Z', '+0200', '{}', '10', 'count')`)
	if err != nil {
		db.Close()
		t.Fatal(err)
	}
	_, err = db.Exec(`INSERT INTO observations (dedup_key, n, kind, source, start_ts, end_ts, start_offset, metadata, value, unit)
		VALUES ('overlap-phone', 1, 'HKQuantityTypeIdentifierStepCount', 'Phone2',
			'2025-09-21T10:00:00.000000000Z', '2025-09-21T10:01:00.000000000Z', '+0200', '{}', '20', 'count')`)
	db.Close()
	if err != nil {
		t.Fatal(err)
	}
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	req := localReq(http.MethodGet, "/sports/stepcount?range=all", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	body := rec.Body.String()
	if rec.Code != 200 {
		t.Fatalf("code %d %s", rec.Code, body)
	}
	if !strings.Contains(body, `name="source"`) {
		t.Fatal("overlapping sources should offer a picker:\n" + body)
	}
	if !strings.Contains(body, ">automatic<") {
		t.Fatal("default choice should be automatic, not a locked device")
	}
	req = localReq(http.MethodGet, "/sports/stepcount?range=all&source=Phone2", nil)
	rec = httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	locked := rec.Body.String()
	if rec.Code != 200 {
		t.Fatalf("lock: %d %s", rec.Code, locked)
	}
	if !strings.Contains(locked, "source=Phone2") {
		t.Fatal("locked range links should keep source=Phone2")
	}
	req = localReq(http.MethodGet, "/sports/stepcount?range=month&end=2026-08-24&source=Apple+Watch", nil)
	rec = httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	kept := rec.Body.String()
	if rec.Code != 200 {
		t.Fatalf("keep end: %d", rec.Code)
	}
	if !strings.Contains(kept, `value="2026-08-24"`) {
		t.Fatal("switching source must keep the selected day:\n" + kept)
	}
	if !strings.Contains(kept, "August 2026") {
		t.Fatal("month window should stay August 2026")
	}
}

func TestPercentHeadlinesArePercentPoints(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	req := localReq(http.MethodGet, "/kind?k=HKQuantityTypeIdentifierWalkingDoubleSupportPercentage", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	body := rec.Body.String()
	if rec.Code != 200 {
		t.Fatalf("code %d %s", rec.Code, body)
	}
	if strings.Contains(body, "0.31") || strings.Contains(body, "0.312") {
		t.Fatal("HealthKit fraction leaked: " + body)
	}
	if !strings.Contains(body, "31.2") {
		t.Fatal("expected 31.2%: " + body)
	}
}

func TestSportsShowsRunningWorkout(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	req := localReq(http.MethodGet, "/sports", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	body := rec.Body.String()
	if rec.Code != 200 {
		t.Fatalf("code %d %s", rec.Code, body)
	}
	if !strings.Contains(body, "Running") {
		t.Fatalf("expected Running in the sports catalog:\n%s", body)
	}
	if !strings.Contains(body, `href="/sports/running"`) {
		t.Fatalf("expected Running to link to /sports/running:\n%s", body)
	}
	if strings.Contains(body, `class="pager"`) || strings.Contains(body, "?on=") {
		t.Fatal("sports is a category list, not a day view")
	}
}

func TestTimelineRedirectsToDomain(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	req := localReq(http.MethodGet, "/timeline?kind=HKWorkoutActivityTypeRunning&range=all", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusFound {
		t.Fatalf("code %d", rec.Code)
	}
	loc := rec.Header().Get("Location")
	if !strings.HasPrefix(loc, "/sports/running") {
		t.Fatalf("location %s", loc)
	}
	if !strings.Contains(loc, "range=all") {
		t.Fatalf("range should survive redirect: %s", loc)
	}
}

func TestDomainPagesHaveNoDateNav(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	h := srv.Handler()
	for _, path := range []string{"/medicine", "/lifestyle", "/sports", "/all"} {
		req := localReq(http.MethodGet, path, nil)
		rec := httptest.NewRecorder()
		h.ServeHTTP(rec, req)
		body := rec.Body.String()
		if rec.Code != 200 {
			t.Fatalf("%s: %d", path, rec.Code)
		}
		if strings.Contains(body, `class="pager"`) {
			t.Fatalf("%s must not have a date pager", path)
		}
		if strings.Contains(body, "?on=") || strings.Contains(body, "?end=") || strings.Contains(body, "&amp;end=") {
			t.Fatalf("%s must not link by calendar day", path)
		}
		if strings.Contains(body, "&amp;range=") || strings.Contains(body, "?kind=") {
			t.Fatalf("%s must not carry range/source filters; those live on /kind", path)
		}
	}
	req := localReq(http.MethodGet, "/", nil)
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if !strings.Contains(rec.Body.String(), `class="pager"`) {
		t.Fatal("Today keeps date navigation")
	}
}

func TestKindDetailOwnsRange(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	h := srv.Handler()
	req := localReq(http.MethodGet, "/sports/activeenergyburned?range=all", nil)
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	body := rec.Body.String()
	if rec.Code != 200 {
		t.Fatalf("code %d %s", rec.Code, body)
	}
	if !strings.Contains(body, "Active energy") {
		t.Fatal(body)
	}
	if !strings.Contains(body, `range=all`) || !strings.Contains(body, `class="active"`) {
		t.Fatal("kind page should highlight the selected range")
	}
	req = localReq(http.MethodGet, "/sports", nil)
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	sports := rec.Body.String()
	if strings.Contains(sports, `class="pager"`) {
		t.Fatal("sports stayed a catalog after visiting /kind")
	}
	if strings.Contains(sports, "&amp;range=all") {
		t.Fatal("range=all must not leak onto sports cards")
	}
	if strings.Contains(sports, "end=") {
		t.Fatal("window end must not leak onto sports cards")
	}
}

func TestKindPageShiftsWindow(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	h := srv.Handler()
	req := localReq(http.MethodGet, "/sports/stepcount?range=7d", nil)
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	body := rec.Body.String()
	if rec.Code != 200 {
		t.Fatalf("code %d %s", rec.Code, body)
	}
	if strings.Contains(body, ">90d<") || strings.Contains(body, "range=90d") {
		t.Fatal("90d should be gone")
	}
	if !strings.Contains(body, ">1d<") || !strings.Contains(body, ">Month<") || !strings.Contains(body, ">Year<") {
		t.Fatal("range chips should be 1d, 7d, Month, Year, All")
	}
	if !strings.Contains(body, `class="pager"`) || !strings.Contains(body, `name="end"`) {
		t.Fatal("7d needs a window pager:\n" + body)
	}
	if !strings.Contains(body, `class="panel"`) {
		t.Fatal("kind chart should sit in a white panel like workout traces")
	}
	if strings.Contains(body, `>Next</a>`) {
		t.Fatal("default window is the latest 7 days; Next should be disabled")
	}
	if !strings.Contains(body, "2026-08-17") {
		t.Fatal("Prev should step the 7d window back:\n" + body)
	}

	req = localReq(http.MethodGet, "/sports/stepcount?range=7d&end=2025-09-16", nil)
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	shifted := rec.Body.String()
	if rec.Code != 200 {
		t.Fatalf("shift: %d", rec.Code)
	}
	if !strings.Contains(shifted, `value="2025-09-16"`) {
		t.Fatal("date picker should land on the requested end:\n" + shifted)
	}
	if !strings.Contains(shifted, "10–16 September 2025") && !strings.Contains(shifted, "September 2025") {
		t.Fatal("window caption should name the chosen 7 days:\n" + shifted)
	}

	req = localReq(http.MethodGet, "/sports/stepcount?range=7d&end=2099-01-01", nil)
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	clamped := rec.Body.String()
	if strings.Contains(clamped, `value="2099-01-01"`) {
		t.Fatal("future end must clamp to the last sample day")
	}

	req = localReq(http.MethodGet, "/sports/stepcount?range=all", nil)
	rec = httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	all := rec.Body.String()
	if strings.Contains(all, `class="pager"`) {
		t.Fatal("all is the full span; no window pager")
	}
}

func TestNoHXBoost(t *testing.T) {
	b, err := tplFS.ReadFile("templates/layout.html")
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(b), "hx-boost") {
		t.Fatal("hx-boost skips script re-run and leaves charts empty")
	}
}

func TestOverviewLinksGaps(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	req := localReq(http.MethodGet, "/", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if !strings.Contains(rec.Body.String(), `href="/gaps"`) {
		t.Fatal("today must link to gaps")
	}
}

func TestAssetsServed(t *testing.T) {
	srv, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	for _, p := range []string{"/assets/app.css", "/assets/app.js", "/assets/vendor/chart.umd.min.js", "/assets/fonts/recursive-latin.woff2"} {
		req := localReq(http.MethodGet, p, nil)
		rec := httptest.NewRecorder()
		srv.Handler().ServeHTTP(rec, req)
		if rec.Code != 200 {
			t.Fatalf("%s: %d", p, rec.Code)
		}
	}
}

func TestMedicineEmptyIsHonest(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	req := localReq(http.MethodGet, "/medicine", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	body := rec.Body.String()
	if strings.Contains(body, "concerning") || strings.Contains(body, "not a medical") {
		t.Fatal(body)
	}
	if !strings.Contains(body, "zone-band") {
		t.Fatal("zone-band component")
	}
}

func TestHostHeaderReject(t *testing.T) {
	srv, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	h := srv.Handler()

	req := httptest.NewRequest(http.MethodGet, "/", nil)
	req.Host = "evil.example"
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("evil.example: %d %s", rec.Code, rec.Body.String())
	}

	for _, host := range []string{"127.0.0.1:8080", "localhost"} {
		req := httptest.NewRequest(http.MethodGet, "/", nil)
		req.Host = host
		rec := httptest.NewRecorder()
		h.ServeHTTP(rec, req)
		if rec.Code != http.StatusOK {
			t.Fatalf("%s: %d %s", host, rec.Code, rec.Body.String())
		}
	}
}

func TestSecFetchSiteCrossSite(t *testing.T) {
	srv, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	req := localReq(http.MethodGet, "/", nil)
	req.Header.Set("Sec-Fetch-Site", "cross-site")
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusForbidden {
		t.Fatalf("cross-site: %d %s", rec.Code, rec.Body.String())
	}
}

func TestSecurityHeaders(t *testing.T) {
	srv, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	req := localReq(http.MethodGet, "/", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("code %d %s", rec.Code, rec.Body.String())
	}
	if rec.Header().Get("X-Content-Type-Options") != "nosniff" {
		t.Fatal("X-Content-Type-Options")
	}
	if rec.Header().Get("Referrer-Policy") != "no-referrer" {
		t.Fatal("Referrer-Policy")
	}
	if rec.Header().Get("X-Frame-Options") != "DENY" {
		t.Fatal("X-Frame-Options")
	}
	csp := rec.Header().Get("Content-Security-Policy")
	if csp == "" {
		t.Fatal("missing CSP")
	}
	m := regexp.MustCompile(`'nonce-([^']+)'`).FindStringSubmatch(csp)
	if m == nil {
		t.Fatalf("CSP missing nonce: %s", csp)
	}
	// html/template escapes '+' in attributes as &#43;
	want := `nonce="` + strings.ReplaceAll(m[1], "+", "&#43;") + `"`
	if !strings.Contains(rec.Body.String(), want) {
		t.Fatalf("HTML missing matching nonce %q", m[1])
	}
}

func TestAssetsNoDirectoryListing(t *testing.T) {
	srv, err := New(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	h := srv.Handler()
	for _, p := range []string{"/assets/", "/assets/vendor/"} {
		req := localReq(http.MethodGet, p, nil)
		rec := httptest.NewRecorder()
		h.ServeHTTP(rec, req)
		if rec.Code != http.StatusNotFound {
			t.Fatalf("%s: %d", p, rec.Code)
		}
	}
	req := localReq(http.MethodGet, "/assets/app.js", nil)
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("/assets/app.js: %d", rec.Code)
	}
}

func TestNewServerTimeouts(t *testing.T) {
	h := http.HandlerFunc(func(http.ResponseWriter, *http.Request) {})
	srv, err := NewServer("127.0.0.1:0", h)
	if err != nil {
		t.Fatal(err)
	}
	if srv.ReadHeaderTimeout == 0 || srv.ReadTimeout == 0 || srv.WriteTimeout == 0 || srv.IdleTimeout == 0 {
		t.Fatalf("timeouts header=%v read=%v write=%v idle=%v",
			srv.ReadHeaderTimeout, srv.ReadTimeout, srv.WriteTimeout, srv.IdleTimeout)
	}
}

func TestInternalErrorHidesDetails(t *testing.T) {
	root := fixtureRoot(t)
	srv, err := New(root)
	if err != nil {
		t.Fatal(err)
	}
	req := localReq(http.MethodGet, "/?on=not-a-date", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusInternalServerError {
		t.Fatalf("code %d %s", rec.Code, rec.Body.String())
	}
	body := rec.Body.String()
	if strings.Contains(body, "day:") || strings.Contains(body, "cannot parse") || strings.Contains(body, "parsing time") {
		t.Fatalf("leaked error: %s", body)
	}
	if !strings.Contains(body, "internal error") {
		t.Fatalf("want generic: %s", body)
	}
}
