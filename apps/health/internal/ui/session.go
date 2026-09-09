package ui

import (
	"encoding/json"
	"fmt"
	"html/template"
	"math"
	"path/filepath"
	"strconv"
	"strings"
	"time"

	"archive/internal/adapters/apple"
	"archive/internal/blobs"
	"archive/internal/projection"
)

type sessionView struct {
	workoutView
	When       string
	Moving     string
	Elapsed    string
	PauseN     int
	Paused     string
	Pace       string
	Ascent     string
	Descent    string
	RouteSVG   template.HTML
	RouteCap   string
	RouteNote  string
	HRMoving   template.JS
	HRElapsed  template.JS
	EleChart   template.JS
	PaceChart  template.JS
	PauseHTML  template.HTML
	PauseLabel string
	Splits     []splitRow
	SourceLine string
}

type splitRow struct {
	N     int
	Label string
	Width int
}

func (s *Server) buildSession(ep projection.EpisodeRow, evs []projection.WorkoutEventRow, hr []projection.ObservationRow) sessionView {
	w := parseWorkout(ep, func(ts string) string {
		d, _ := localDay(ts, s.loc)
		return d
	})
	w.Href = ""
	view := sessionView{workoutView: w}
	start, _ := time.Parse(time.RFC3339Nano, ep.Start)
	end, _ := time.Parse(time.RFC3339Nano, ep.End)
	if !start.IsZero() {
		view.When = start.In(s.loc).Format("Monday 2 January · 15:04")
		if !end.IsZero() {
			view.When += "–" + end.In(s.loc).Format("15:04")
		}
		if w.Source != "" {
			view.When += " · " + w.Source
		}
	}
	moving, elapsed, paused := routeTimes(ep, start, end)
	view.Moving = formatClock(moving)
	view.Elapsed = formatClock(elapsed)
	if ep.PauseCount != nil {
		view.PauseN = *ep.PauseCount
	}
	view.Paused = formatClock(paused)
	if km, ok := parseKm(w.Distance); ok && moving > 0 && km > 0 {
		view.Pace = formatPace(float64(moving) / km)
	}
	if ep.AscentM != nil {
		view.Ascent = trimFloat(*ep.AscentM) + " m"
	}
	if ep.DescentM != nil {
		view.Descent = trimFloat(*ep.DescentM) + " m"
	}

	if len(ep.RoutePolyline) > 0 {
		if segs, err := projection.DecodePolyline(ep.RoutePolyline); err == nil {
			view.RouteSVG = routeSVG(segs)
			view.RouteCap = fmt.Sprintf("%s · %d segment%s · simplified to %d points",
				orDash(w.Distance), ep.RouteSegments, plural(ep.RouteSegments), ep.RoutePoints)
		}
	} else if w.HasRoute {
		view.RouteNote = "A route file is stored. Rebuild the projection to decode it into a polyline."
	}

	track := s.loadTrack(ep)
	pauses := pauseWindows(evs)
	view.HRMoving = hrChart(hr, start, end, pauses, true)
	view.HRElapsed = hrChart(hr, start, end, pauses, false)
	if len(track) > 0 {
		view.EleChart = elevationChart(track)
		view.PaceChart = paceChart(track)
		view.Splits = kmSplits(track)
	}
	view.PauseHTML, view.PauseLabel = pauseStrip(start, elapsed, pauses, moving)
	view.SourceLine = sourceLine(ep)
	return view
}

func routeTimes(ep projection.EpisodeRow, start, end time.Time) (moving, elapsed, paused int) {
	if ep.ElapsedSeconds != nil {
		elapsed = *ep.ElapsedSeconds
	} else if !start.IsZero() && !end.IsZero() && !end.Before(start) {
		elapsed = int(end.Sub(start).Seconds())
	}
	if ep.MovingSeconds != nil {
		moving = *ep.MovingSeconds
	} else {
		moving = elapsed
	}
	paused = elapsed - moving
	if paused < 0 {
		paused = 0
	}
	return moving, elapsed, paused
}

func (s *Server) loadTrack(ep projection.EpisodeRow) []apple.TrackPoint {
	path := routeFileName(ep.Body)
	if path == "" || ep.BlobSHA256 == "" {
		return nil
	}
	blobPath, err := blobs.Path(s.Root, ep.BlobSHA256)
	if err != nil {
		return nil
	}
	rc, err := apple.OpenMember(blobPath, path)
	if err != nil {
		return nil
	}
	defer rc.Close()
	pts, err := apple.ParseGPX(rc)
	if err != nil {
		return nil
	}
	return pts
}

func routeFileName(body string) string {
	var m map[string]any
	if json.Unmarshal([]byte(body), &m) != nil {
		return ""
	}
	routes, ok := m["routes"].([]any)
	if !ok {
		return ""
	}
	for _, r := range routes {
		mm, ok := r.(map[string]any)
		if !ok {
			continue
		}
		if p := asString(mm["path"]); p != "" {
			return p
		}
	}
	return ""
}

func sourceLine(ep projection.EpisodeRow) string {
	var parts []string
	if ep.BlobSHA256 != "" {
		parts = append(parts, "sha256 "+ep.BlobSHA256[:6]+"…")
	}
	if p := routeFileName(ep.Body); p != "" {
		parts = append(parts, filepath.Base(p))
	}
	return strings.Join(parts, " · ")
}

type pauseWin struct {
	From time.Time
	To   time.Time
	Auto bool
}

func pauseWindows(evs []projection.WorkoutEventRow) []pauseWin {
	var out []pauseWin
	var start *time.Time
	auto := false
	for _, ev := range evs {
		t, err := time.Parse(time.RFC3339Nano, ev.Time)
		if err != nil {
			continue
		}
		if ev.Kind == "HKWorkoutEventTypePause" || ev.Kind == "HKWorkoutEventTypeMotionPaused" {
			tt := t
			start = &tt
			auto = ev.Kind == "HKWorkoutEventTypeMotionPaused"
			continue
		}
		if (ev.Kind == "HKWorkoutEventTypeResume" || ev.Kind == "HKWorkoutEventTypeMotionResumed") && start != nil {
			if ev.Kind == "HKWorkoutEventTypeMotionResumed" {
				auto = true
			}
			out = append(out, pauseWin{From: *start, To: t, Auto: auto})
			start = nil
			auto = false
		}
	}
	return out
}

func inPause(t time.Time, pauses []pauseWin) bool {
	for _, p := range pauses {
		if !t.Before(p.From) && t.Before(p.To) {
			return true
		}
	}
	return false
}

func movingOffset(t, start time.Time, pauses []pauseWin) time.Duration {
	if t.Before(start) {
		return 0
	}
	off := t.Sub(start)
	for _, p := range pauses {
		if p.To.Before(start) || !p.From.Before(t) {
			continue
		}
		a, b := p.From, p.To
		if a.Before(start) {
			a = start
		}
		if b.After(t) {
			b = t
		}
		if b.After(a) {
			off -= b.Sub(a)
		}
	}
	if off < 0 {
		return 0
	}
	return off
}

func hrChart(rows []projection.ObservationRow, start, end time.Time, pauses []pauseWin, movingOnly bool) template.JS {
	var labels []string
	var data []*float64
	for _, r := range rows {
		t, err := time.Parse(time.RFC3339Nano, r.Start)
		if err != nil {
			continue
		}
		v, ok := parseNum(r.Value)
		if !ok {
			continue
		}
		paused := inPause(t, pauses)
		if movingOnly && paused {
			continue
		}
		var x time.Duration
		if movingOnly {
			x = movingOffset(t, start, pauses)
		} else {
			x = t.Sub(start)
		}
		if x < 0 {
			continue
		}
		val := v
		if !movingOnly && paused {
			val = v // keep the sample so elapsed shows rest HR
		}
		labels = append(labels, formatClock(int(x.Seconds())))
		data = append(data, &val)
	}
	labels, data = downsampleSeries(labels, data, 160)
	if len(data) < 2 {
		return ""
	}
	xTitle := "Moving time"
	if !movingOnly {
		xTitle = "Elapsed time"
	}
	return marshalChart(chartCfg{
		Type:     "line",
		Labels:   labels,
		Datasets: []chartDS{{Label: "Heart rate", Data: data}},
		XTitle:   xTitle,
		YTitle:   "bpm",
		Color:    "#B3123F",
	})
}

func elevationChart(track []apple.TrackPoint) template.JS {
	var labels []string
	var data []*float64
	var dist float64
	var prev *apple.TrackPoint
	for i := range track {
		p := &track[i]
		if prev != nil {
			dist += equirect(prev.Lat, prev.Lon, p.Lat, p.Lon)
		}
		if p.Ele != nil {
			v := *p.Ele
			labels = append(labels, trimFloat(dist/1000)+" km")
			data = append(data, &v)
		}
		prev = p
	}
	labels, data = downsampleSeries(labels, data, 160)
	if len(data) < 2 {
		return ""
	}
	return marshalChart(chartCfg{
		Type:     "line",
		Labels:   labels,
		Datasets: []chartDS{{Label: "Elevation", Data: data}},
		XTitle:   "Distance",
		YTitle:   "m",
		Color:    "#1D6F42",
	})
}

func paceChart(track []apple.TrackPoint) template.JS {
	const winM = 200.0
	type acc struct {
		d float64
		t time.Time
	}
	var hist []acc
	var dist float64
	var labels []string
	var data []*float64
	for i := 1; i < len(track); i++ {
		a, b := track[i-1], track[i]
		if a.Time.IsZero() || b.Time.IsZero() || !b.Time.After(a.Time) {
			continue
		}
		dd := equirect(a.Lat, a.Lon, b.Lat, b.Lon)
		dist += dd
		hist = append(hist, acc{d: dist, t: b.Time})
		for len(hist) > 1 && dist-hist[0].d > winM {
			hist = hist[1:]
		}
		if len(hist) < 2 {
			continue
		}
		dd = hist[len(hist)-1].d - hist[0].d
		dt := hist[len(hist)-1].t.Sub(hist[0].t).Seconds()
		if dd < 20 || dt <= 0 {
			continue
		}
		secPerKm := dt / (dd / 1000)
		if secPerKm < 60 || secPerKm > 30*60 {
			continue
		}
		v := secPerKm
		labels = append(labels, trimFloat(dist/1000)+" km")
		data = append(data, &v)
	}
	labels, data = downsampleSeries(labels, data, 160)
	if len(data) < 2 {
		return ""
	}
	return marshalChart(chartCfg{
		Type:     "line",
		Labels:   labels,
		Datasets: []chartDS{{Label: "Pace", Data: data}},
		XTitle:   "Distance",
		YTitle:   "min/km",
		YFormat:  "pace",
		Color:    "#146B68",
	})
}

func downsampleSeries(labels []string, data []*float64, n int) ([]string, []*float64) {
	if len(data) <= n || n < 2 {
		return labels, data
	}
	outL := make([]string, n)
	outD := make([]*float64, n)
	last := len(data) - 1
	for i := 0; i < n; i++ {
		j := i * last / (n - 1)
		outL[i] = labels[j]
		outD[i] = data[j]
	}
	return outL, outD
}

func kmSplits(track []apple.TrackPoint) []splitRow {
	if len(track) < 2 {
		return nil
	}
	var dist float64
	var t0 time.Time
	if !track[0].Time.IsZero() {
		t0 = track[0].Time
	}
	var splits []float64
	next := 1000.0
	for i := 1; i < len(track); i++ {
		dist += equirect(track[i-1].Lat, track[i-1].Lon, track[i].Lat, track[i].Lon)
		if dist >= next && !track[i].Time.IsZero() && !t0.IsZero() {
			splits = append(splits, track[i].Time.Sub(t0).Seconds())
			t0 = track[i].Time
			next += 1000
		}
	}
	if len(splits) == 0 {
		return nil
	}
	max := splits[0]
	for _, s := range splits {
		if s > max {
			max = s
		}
	}
	out := make([]splitRow, len(splits))
	for i, s := range splits {
		w := 20
		if max > 0 {
			w = int(math.Round(100 * s / max))
			if w < 8 {
				w = 8
			}
		}
		out[i] = splitRow{N: i + 1, Label: formatPace(s), Width: w}
	}
	return out
}

func pauseStrip(start time.Time, elapsed int, pauses []pauseWin, moving int) (template.HTML, string) {
	if elapsed <= 0 {
		return "", ""
	}
	type seg struct {
		Sec  int
		Stop bool
		Auto bool
	}
	var segs []seg
	cursor := 0
	paused, autoN, manN := 0, 0, 0
	if !start.IsZero() {
		for _, p := range pauses {
			from := int(p.From.Sub(start).Seconds())
			to := int(p.To.Sub(start).Seconds())
			if from < 0 {
				from = 0
			}
			if to > elapsed {
				to = elapsed
			}
			if to <= from {
				continue
			}
			if from > cursor {
				segs = append(segs, seg{Sec: from - cursor})
			}
			segs = append(segs, seg{Sec: to - from, Stop: true, Auto: p.Auto})
			paused += to - from
			if p.Auto {
				autoN++
			} else {
				manN++
			}
			cursor = to
		}
	}
	if cursor < elapsed {
		segs = append(segs, seg{Sec: elapsed - cursor})
	}
	if len(segs) == 0 && moving > 0 && moving < elapsed {
		paused = elapsed - moving
		segs = []seg{{Sec: moving}, {Sec: paused, Stop: true}}
	}
	if len(segs) == 0 {
		return "", ""
	}
	var b strings.Builder
	b.WriteString(`<div class="pause-strip" title="Grey is paused time. Auto-pause (MotionPaused) and manual Pause both count; only pauses of 60s or more split the route.">`)
	for _, s := range segs {
		cls := "run"
		if s.Stop {
			cls = "stop"
			if s.Auto {
				cls += " auto"
			}
		}
		fmt.Fprintf(&b, `<span class="%s" style="flex:%d"></span>`, cls, maxInt(s.Sec, 1))
	}
	b.WriteString(`</div>`)
	label := fmt.Sprintf("Paused %s total", formatClock(paused))
	switch {
	case autoN > 0 && manN > 0:
		label += fmt.Sprintf(" · %d auto, %d manual", autoN, manN)
	case autoN > 0:
		label += fmt.Sprintf(" · %d auto-pause%s", autoN, plural(autoN))
	case manN > 0:
		label += fmt.Sprintf(" · %d pause%s", manN, plural(manN))
	}
	return template.HTML(b.String()), label
}

func routeSVG(segs [][][2]float32) template.HTML {
	var minLat, maxLat, minLon, maxLon float32
	first := true
	n := 0
	for _, s := range segs {
		for _, p := range s {
			if first {
				minLat, maxLat, minLon, maxLon = p[0], p[0], p[1], p[1]
				first = false
			}
			if p[0] < minLat {
				minLat = p[0]
			}
			if p[0] > maxLat {
				maxLat = p[0]
			}
			if p[1] < minLon {
				minLon = p[1]
			}
			if p[1] > maxLon {
				maxLon = p[1]
			}
			n++
		}
	}
	if n < 2 {
		return ""
	}
	const W, H, pad = 300.0, 170.0, 12.0
	dlat := float64(maxLat - minLat)
	dlon := float64(maxLon - minLon)
	if dlat == 0 {
		dlat = 1e-5
	}
	if dlon == 0 {
		dlon = 1e-5
	}
	// Fit lon/lat into the box preserving aspect (approx metres).
	lat0 := float64(minLat+maxLat) / 2 * math.Pi / 180
	sx := dlon * math.Cos(lat0)
	sy := dlat
	innerW, innerH := W-2*pad, H-2*pad
	scale := innerW / sx
	if innerH/sy < scale {
		scale = innerH / sy
	}
	offX := pad + (innerW-sx*scale)/2
	offY := pad + (innerH-sy*scale)/2
	proj := func(lat, lon float32) (float64, float64) {
		x := offX + (float64(lon)-float64(minLon))*math.Cos(lat0)*scale
		y := offY + (float64(maxLat)-float64(lat))*scale
		return x, y
	}
	var b strings.Builder
	b.WriteString(`<svg viewBox="0 0 300 170" class="route-svg" role="img" aria-label="Route">`)
	var startX, startY, endX, endY float64
	started := false
	for _, s := range segs {
		if len(s) == 0 {
			continue
		}
		b.WriteString(`<polyline fill="none" class="route-line" stroke-width="2.5" stroke-linejoin="round" stroke-linecap="round" points="`)
		for i, p := range s {
			x, y := proj(p[0], p[1])
			if !started {
				startX, startY = x, y
				started = true
			}
			endX, endY = x, y
			if i > 0 {
				b.WriteByte(' ')
			}
			fmt.Fprintf(&b, "%.1f,%.1f", x, y)
		}
		b.WriteString(`"/>`)
	}
	fmt.Fprintf(&b, `<circle class="route-start" cx="%.1f" cy="%.1f" r="4.5"/>`, startX, startY)
	fmt.Fprintf(&b, `<circle class="route-end" cx="%.1f" cy="%.1f" r="4.5"/>`, endX, endY)
	b.WriteString(`</svg>`)
	return template.HTML(b.String())
}

func equirect(lat1, lon1, lat2, lon2 float64) float64 {
	const metresPerDeg = 111320.0
	lat0 := (lat1 + lat2) / 2 * math.Pi / 180
	dx := (lon2 - lon1) * math.Cos(lat0) * metresPerDeg
	dy := (lat2 - lat1) * metresPerDeg
	return math.Hypot(dx, dy)
}

func formatClock(sec int) string {
	if sec < 0 {
		sec = 0
	}
	h := sec / 3600
	m := (sec % 3600) / 60
	s := sec % 60
	if h > 0 {
		return fmt.Sprintf("%d:%02d:%02d", h, m, s)
	}
	return fmt.Sprintf("%d:%02d", m, s)
}

func formatPace(secPerKm float64) string {
	if secPerKm <= 0 || math.IsInf(secPerKm, 0) || math.IsNaN(secPerKm) {
		return ""
	}
	sec := int(math.Round(secPerKm))
	return fmt.Sprintf("%d:%02d", sec/60, sec%60)
}

func parseKm(s string) (float64, bool) {
	s = strings.TrimSpace(strings.ToLower(s))
	s = strings.TrimSuffix(s, "km")
	s = strings.TrimSpace(s)
	v, err := strconv.ParseFloat(s, 64)
	return v, err == nil && v > 0
}

func orDash(s string) string {
	if s == "" {
		return "—"
	}
	return s
}

func plural(n int) string {
	if n == 1 {
		return ""
	}
	return "s"
}

func maxInt(a, b int) int {
	if a > b {
		return a
	}
	return b
}
