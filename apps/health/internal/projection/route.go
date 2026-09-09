package projection

import (
	"database/sql"
	"encoding/binary"
	"fmt"
	"math"
	"os"
	"sort"
	"strconv"
	"strings"
	"time"

	"archive/config"
	"archive/internal/adapters/apple"
)

// PolylineMagic is the 4-byte header of route_polyline (ASCII "ARPL").
const PolylineMagic = "ARPL"

const (
	polylineVersion byte = 1
	// implausibleSpeedMps splits a segment when consecutive points imply
	// faster motion than this (GPS teleport). 50 m/s = 180 km/h.
	implausibleSpeedMps = 50
	rdpToleranceM       = 10
	metresPerDeg        = 111320.0
	eleHysteresisM      = 3.0
	maxPolylineU16      = 65535
)

// RouteResult is the derived route + timing for one workout episode.
type RouteResult struct {
	Polyline []byte
	Segments int
	Points   int
	BBox     string
	AscentM  *float64
	DescentM *float64
	MovingS  int
	ElapsedS int
	PauseN   int
	SplitAt  []time.Time // pause starts that actually split the route
	PausedS  int
	Warnings []string
}

type xy struct {
	x, y float64
	i    int
}

// WorkoutEventKind is the Apple WorkoutEvent type identifier, stored verbatim.
type projectedEvent struct {
	Kind string
	Time time.Time
}

func projectWorkoutEvents(raw []map[string]string) []projectedEvent {
	var out []projectedEvent
	for _, ev := range raw {
		kind := ev["type"]
		if kind == "" {
			continue
		}
		utc, _, err := apple.ParseTime(ev["date"])
		if err != nil {
			continue
		}
		t, err := time.Parse(time.RFC3339Nano, utc)
		if err != nil {
			continue
		}
		out = append(out, projectedEvent{Kind: kind, Time: t})
	}
	return out
}

func isPauseKind(k string) bool {
	return k == "HKWorkoutEventTypePause" || k == "HKWorkoutEventTypeMotionPaused"
}

func isResumeKind(k string) bool {
	return k == "HKWorkoutEventTypeResume" || k == "HKWorkoutEventTypeMotionResumed"
}

// timeFromWorkout derives moving/elapsed/pause_count from start/end and events.
func timeFromWorkout(start, end time.Time, evs []projectedEvent, thresh time.Duration) (elapsed, moving, paused, pauseN int, splits []time.Time) {
	if !start.IsZero() && !end.IsZero() && !end.Before(start) {
		elapsed = int(end.Sub(start).Seconds())
	}
	evs = append([]projectedEvent(nil), evs...)
	sort.Slice(evs, func(i, j int) bool {
		if evs[i].Time.Equal(evs[j].Time) {
			return evs[i].Kind < evs[j].Kind
		}
		return evs[i].Time.Before(evs[j].Time)
	})
	var pauseStart *time.Time
	closePause := func(at time.Time) {
		if pauseStart == nil {
			return
		}
		d := at.Sub(*pauseStart)
		if d < 0 {
			d = 0
		}
		paused += int(d.Seconds())
		pauseN++
		if d >= thresh {
			splits = append(splits, *pauseStart)
		}
		pauseStart = nil
	}
	for _, ev := range evs {
		if isPauseKind(ev.Kind) {
			if pauseStart != nil {
				continue
			}
			t := ev.Time
			pauseStart = &t
			continue
		}
		if isResumeKind(ev.Kind) && pauseStart != nil {
			closePause(ev.Time)
		}
	}
	if pauseStart != nil && !end.IsZero() {
		closePause(end)
	}
	moving = elapsed - paused
	if moving < 0 {
		moving = 0
	}
	return elapsed, moving, paused, pauseN, splits
}

func deriveRoute(pts []apple.TrackPoint, evs []projectedEvent, start, end time.Time) RouteResult {
	thresh := config.PauseThreshold()
	elapsed, moving, paused, pauseN, eventSplits := timeFromWorkout(start, end, evs, thresh)
	r := RouteResult{
		MovingS:  moving,
		ElapsedS: elapsed,
		PauseN:   pauseN,
		PausedS:  paused,
		SplitAt:  eventSplits,
	}
	if len(pts) == 0 {
		return r
	}
	ascent, descent := elevationChange(pts)
	r.AscentM = ascent
	r.DescentM = descent

	segs := splitTrack(pts, thresh, eventSplits)
	var kept [][]apple.TrackPoint
	for _, s := range segs {
		s = rdpSegment(s, rdpToleranceM)
		if len(s) == 0 {
			continue
		}
		kept = append(kept, s)
	}
	if len(kept) == 0 {
		return r
	}
	r.Segments = len(kept)
	for _, s := range kept {
		r.Points += len(s)
	}
	r.BBox = bbox(kept)
	poly, warns := encodePolyline(kept)
	r.Polyline = poly
	r.Warnings = append(r.Warnings, warns...)
	return r
}

func elevationChange(pts []apple.TrackPoint) (ascent, descent *float64) {
	var a, d float64
	var ref *float64
	var any bool
	for _, p := range pts {
		if p.Ele == nil {
			continue
		}
		any = true
		if ref == nil {
			v := *p.Ele
			ref = &v
			continue
		}
		delta := *p.Ele - *ref
		if delta >= eleHysteresisM || delta <= -eleHysteresisM {
			if delta > 0 {
				a += delta
			} else {
				d += -delta
			}
			v := *p.Ele
			ref = &v
		}
	}
	if !any {
		return nil, nil
	}
	return &a, &d
}

func splitTrack(pts []apple.TrackPoint, thresh time.Duration, eventSplits []time.Time) [][]apple.TrackPoint {
	if len(pts) == 0 {
		return nil
	}
	var segs [][]apple.TrackPoint
	cur := []apple.TrackPoint{pts[0]}
	for i := 1; i < len(pts); i++ {
		prev, p := pts[i-1], pts[i]
		split := false
		if !prev.Time.IsZero() && !p.Time.IsZero() {
			dt := p.Time.Sub(prev.Time)
			if dt >= thresh {
				split = true
			} else if dt > 0 {
				dist := equirectDist(prev.Lat, prev.Lon, p.Lat, p.Lon)
				if dist/dt.Seconds() > implausibleSpeedMps {
					split = true
				}
			}
		}
		if !split && crossesEventSplit(prev.Time, p.Time, eventSplits) {
			split = true
		}
		if split {
			if len(cur) > 0 {
				segs = append(segs, cur)
			}
			cur = []apple.TrackPoint{p}
			continue
		}
		cur = append(cur, p)
	}
	if len(cur) > 0 {
		segs = append(segs, cur)
	}
	return segs
}

func crossesEventSplit(a, b time.Time, splits []time.Time) bool {
	if a.IsZero() || b.IsZero() {
		return false
	}
	for _, s := range splits {
		if s.After(a) && !s.After(b) {
			return true
		}
	}
	return false
}

func equirectDist(lat1, lon1, lat2, lon2 float64) float64 {
	lat0 := (lat1 + lat2) / 2 * math.Pi / 180
	dx := (lon2 - lon1) * math.Cos(lat0) * metresPerDeg
	dy := (lat2 - lat1) * metresPerDeg
	return math.Hypot(dx, dy)
}

func rdpSegment(pts []apple.TrackPoint, tolM float64) []apple.TrackPoint {
	if len(pts) <= 2 {
		return pts
	}
	lat0 := pts[0].Lat * math.Pi / 180
	cosLat := math.Cos(lat0)
	proj := make([]xy, len(pts))
	for i, p := range pts {
		proj[i] = xy{x: p.Lon * cosLat * metresPerDeg, y: p.Lat * metresPerDeg, i: i}
	}
	keep := make([]bool, len(pts))
	rdp(proj, 0, len(proj)-1, tolM, keep)
	keep[0], keep[len(keep)-1] = true, true
	var out []apple.TrackPoint
	for i, p := range pts {
		if keep[i] {
			out = append(out, p)
		}
	}
	return out
}

func rdp(pts []xy, start, end int, tol float64, keep []bool) {
	if end <= start+1 {
		return
	}
	maxD := -1.0
	maxI := start
	for i := start + 1; i < end; i++ {
		d := perpDist(pts[i], pts[start], pts[end])
		if d > maxD {
			maxD = d
			maxI = i
		}
	}
	if maxD > tol {
		keep[maxI] = true
		rdp(pts, start, maxI, tol, keep)
		rdp(pts, maxI, end, tol, keep)
	}
}

func perpDist(p, a, b xy) float64 {
	dx, dy := b.x-a.x, b.y-a.y
	if dx == 0 && dy == 0 {
		return math.Hypot(p.x-a.x, p.y-a.y)
	}
	t := ((p.x-a.x)*dx + (p.y-a.y)*dy) / (dx*dx + dy*dy)
	if t < 0 {
		t = 0
	} else if t > 1 {
		t = 1
	}
	return math.Hypot(p.x-(a.x+t*dx), p.y-(a.y+t*dy))
}

func bbox(segs [][]apple.TrackPoint) string {
	minLat, minLon := 90.0, 180.0
	maxLat, maxLon := -90.0, -180.0
	n := 0
	for _, s := range segs {
		for _, p := range s {
			n++
			if p.Lat < minLat {
				minLat = p.Lat
			}
			if p.Lat > maxLat {
				maxLat = p.Lat
			}
			if p.Lon < minLon {
				minLon = p.Lon
			}
			if p.Lon > maxLon {
				maxLon = p.Lon
			}
		}
	}
	if n == 0 {
		return ""
	}
	return trimCoord(minLat) + "," + trimCoord(minLon) + "," + trimCoord(maxLat) + "," + trimCoord(maxLon)
}

func trimCoord(v float64) string {
	return strconv.FormatFloat(v, 'f', 6, 64)
}

// encodePolyline writes ARPL v1 little-endian:
//
//	magic[4] "ARPL"
//	version u8 = 1
//	flags   u8 = 0
//	nseg    u16
//	for each segment:
//	  npts u16
//	  for each point: lat f32, lon f32
func encodePolyline(segs [][]apple.TrackPoint) ([]byte, []string) {
	var warnings []string
	var chunks [][]apple.TrackPoint
	for _, s := range segs {
		for len(s) > maxPolylineU16 {
			chunks = append(chunks, s[:maxPolylineU16:maxPolylineU16])
			s = s[maxPolylineU16:]
		}
		if len(s) > 0 {
			chunks = append(chunks, s)
		}
	}
	if len(chunks) > maxPolylineU16 {
		warnings = append(warnings, "polyline: more than 65535 segments; truncated")
		chunks = chunks[:maxPolylineU16]
	}
	n := 8
	for _, s := range chunks {
		n += 2 + 8*len(s)
	}
	buf := make([]byte, n)
	copy(buf[0:4], PolylineMagic)
	buf[4] = polylineVersion
	buf[5] = 0
	binary.LittleEndian.PutUint16(buf[6:8], uint16(len(chunks)))
	off := 8
	for _, s := range chunks {
		binary.LittleEndian.PutUint16(buf[off:off+2], uint16(len(s)))
		off += 2
		for _, p := range s {
			binary.LittleEndian.PutUint32(buf[off:off+4], math.Float32bits(float32(p.Lat)))
			binary.LittleEndian.PutUint32(buf[off+4:off+8], math.Float32bits(float32(p.Lon)))
			off += 8
		}
	}
	return buf[:off], warnings
}

// DecodePolyline reads an ARPL v1 blob. Used by tests and a future UI.
func DecodePolyline(b []byte) ([][][2]float32, error) {
	if len(b) < 8 || string(b[:4]) != PolylineMagic || b[4] != polylineVersion {
		return nil, errString("route_polyline: bad header")
	}
	nseg := int(binary.LittleEndian.Uint16(b[6:8]))
	off := 8
	var segs [][][2]float32
	for i := 0; i < nseg; i++ {
		if off+2 > len(b) {
			return nil, errString("route_polyline: truncated segment")
		}
		npts := int(binary.LittleEndian.Uint16(b[off : off+2]))
		off += 2
		seg := make([][2]float32, npts)
		for j := 0; j < npts; j++ {
			if off+8 > len(b) {
				return nil, errString("route_polyline: truncated point")
			}
			seg[j][0] = math.Float32frombits(binary.LittleEndian.Uint32(b[off : off+4]))
			seg[j][1] = math.Float32frombits(binary.LittleEndian.Uint32(b[off+4 : off+8]))
			off += 8
		}
		segs = append(segs, seg)
	}
	return segs, nil
}

type errString string

func (e errString) Error() string { return string(e) }

func parseEpisodeTime(s string) time.Time {
	t, err := time.Parse(time.RFC3339Nano, s)
	if err != nil {
		t, err = time.Parse(time.RFC3339, s)
		if err != nil {
			return time.Time{}
		}
	}
	return t.UTC()
}

func enrichEpisode(blobPath string, e apple.Episode) RouteResult {
	if !strings.HasPrefix(e.Kind, "HKWorkoutActivityType") {
		return RouteResult{}
	}
	evs := projectWorkoutEvents(asStringMaps(e.Body["events"]))
	start := parseEpisodeTime(e.Start)
	end := parseEpisodeTime(e.End)
	var pts []apple.TrackPoint
	var warnings []string
	for _, r := range asStringMaps(e.Body["routes"]) {
		p := strings.TrimSpace(r["path"])
		if p == "" {
			continue
		}
		rc, err := apple.OpenMember(blobPath, p)
		if err != nil {
			warnings = append(warnings, fmt.Sprintf("route %s: %v", p, err))
			fmt.Fprintf(os.Stderr, "warn: route %s: %v\n", p, err)
			continue
		}
		got, err := apple.ParseGPX(rc)
		rc.Close()
		if err != nil {
			warnings = append(warnings, fmt.Sprintf("route %s: %v", p, err))
			fmt.Fprintf(os.Stderr, "warn: route %s: %v\n", p, err)
			continue
		}
		pts = append(pts, got...)
	}
	rt := deriveRoute(pts, evs, start, end)
	if len(warnings) > 0 {
		rt.Warnings = append(warnings, rt.Warnings...)
	}
	return rt
}

func insertWorkoutEvents(ins *sql.Stmt, e apple.Episode) error {
	evs := asStringMaps(e.Body["events"])
	for i, ev := range evs {
		kind := ev["type"]
		if kind == "" {
			continue
		}
		utc, _, err := apple.ParseTime(ev["date"])
		if err != nil {
			continue
		}
		if _, err := ins.Exec(e.DedupKey, i, kind, utc); err != nil {
			return err
		}
	}
	return nil
}

func nullBlob(b []byte) any {
	if len(b) == 0 {
		return nil
	}
	return b
}

func nullInt(v int, ok bool) any {
	if !ok {
		return nil
	}
	return v
}

func nullStr(s string) any {
	if s == "" {
		return nil
	}
	return s
}

func nullFloat(p *float64) any {
	if p == nil {
		return nil
	}
	return *p
}

func routePathFromBody(routes []map[string]string) string {
	for _, r := range routes {
		if p := strings.TrimSpace(r["path"]); p != "" {
			return p
		}
	}
	return ""
}

func asStringMaps(v any) []map[string]string {
	switch t := v.(type) {
	case []map[string]string:
		return t
	case []any:
		var out []map[string]string
		for _, raw := range t {
			m, ok := raw.(map[string]string)
			if !ok {
				if gm, ok := raw.(map[string]any); ok {
					m = map[string]string{}
					for k, val := range gm {
						if s, ok := val.(string); ok {
							m[k] = s
						}
					}
				} else {
					continue
				}
			}
			out = append(out, m)
		}
		return out
	default:
		return nil
	}
}
