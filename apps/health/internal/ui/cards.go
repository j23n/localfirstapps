package ui

import (
	"database/sql"
	"fmt"
	"html/template"
	"math"
	"net/url"
	"strings"
	"time"

	"archive/config"
	"archive/internal/projection"
)

type kindCard struct {
	Kind      string
	Display   string
	Domain    string
	Hue       string
	Value     string
	Unit      string
	Source    string // provenance of the headline, after preferRows
	Filter    string // locked ?source= value; empty means automatic
	Locked    bool
	Multi     bool
	Overlaps  bool
	Sources   []string
	Note      string
	AsOf      string
	Href      string
	Spark     template.HTML
	ChartJSON template.JS
	Range     string
	Grain     string
	End       string
	First     string
	Last      string
	PrevEnd   string
	NextEnd   string
	Window    string
	HasPager  bool
	Unclass   bool
	Rows      []seriesRow
}

func (c kindCard) Link(rng, end string) string {
	src := ""
	if c.Locked {
		src = c.Filter
	}
	if rng == "all" {
		end = ""
	}
	return c.Href + kindQuery(rng, end, src)
}

type seriesRow struct {
	Day    string
	Value  string
	Unit   string
	Source string
}

func domainHue(domain string) string {
	switch domain {
	case config.DomainMedicine, config.DomainLifestyle, config.DomainSports:
		return domain
	default:
		return config.DomainOther
	}
}

func sourceLock(info projection.KindInfo, want string) string {
	if want == "" || want == "all" || !info.Overlaps {
		return ""
	}
	if info.HasSource(want) {
		return want
	}
	return ""
}

func (s *Server) baseCard(info projection.KindInfo, lock string) kindCard {
	sp := config.Lookup(info.Kind)
	src := lock
	if src == "" {
		src = preferredSource(info)
	}
	return kindCard{
		Kind:     info.Kind,
		Display:  sp.Display,
		Domain:   sp.Domain,
		Hue:      categoryOf(info.Kind),
		Source:   src,
		Filter:   lock,
		Locked:   lock != "",
		Multi:    len(info.Sources) > 1,
		Overlaps: info.Overlaps,
		Sources:  info.Sources,
		Unclass:  sp.Fallback,
		Unit:     info.Unit,
		Href:     kindPath(info.Kind),
	}
}

// loadDayCard is Today: that local day's composite (preferred on a tie).
func (s *Server) loadDayCard(db *sql.DB, info projection.KindInfo, on string) (kindCard, error) {
	c := s.baseCard(info, "")
	if info.Table == "episodes" {
		return s.loadDayEpisode(db, info, c, on)
	}
	sp := config.Lookup(info.Kind)
	pref := preferredSource(info)
	win, err := projection.LocalWindow(on, "", "", s.loc)
	if err != nil {
		return c, err
	}
	rows, err := projection.ListObservations(db, projection.ObsQuery{Kind: info.Kind, Window: win})
	if err != nil {
		return c, err
	}
	days := compositeDays(rows, "", pref, s.loc, sp.Aggregate)
	c.Source = ""
	if len(days) > 0 {
		today := days[len(days)-1]
		c.Value, c.Unit = formatAmount(today.Value, today.Unit)
		c.Source = today.Source
		bf, bt := baselineWindow(on, s.loc)
		bwin, err := projection.LocalWindow("", bf, bt, s.loc)
		if err != nil {
			return c, err
		}
		histRows, err := projection.ListObservations(db, projection.ObsQuery{Kind: info.Kind, Window: bwin})
		if err != nil {
			return c, err
		}
		var hist []float64
		for _, d := range compositeDays(histRows, "", pref, s.loc, sp.Aggregate) {
			hist = append(hist, d.Value)
		}
		c.Note = BaselineNote(today.Value, hist)
	}
	return c, nil
}

func (s *Server) loadDayEpisode(db *sql.DB, info projection.KindInfo, c kindCard, on string) (kindCard, error) {
	win, err := projection.LocalWindow(on, "", "", s.loc)
	if err != nil {
		return c, err
	}
	rows, err := projection.ListEpisodes(db, projection.EpQuery{Kind: info.Kind, Window: win})
	if err != nil {
		return c, err
	}
	if n := len(rows); n > 0 {
		c.Value = fmt.Sprintf("%d", n)
		c.Unit = "sessions"
	}
	return c, nil
}

// loadCatalogCard is a section row: last daily composite value, dated.
func (s *Server) loadCatalogCard(db *sql.DB, info projection.KindInfo) (kindCard, error) {
	c := s.baseCard(info, "")
	if info.Table == "episodes" {
		return s.loadCatalogEpisode(db, info, c)
	}
	sp := config.Lookup(info.Kind)
	pref := preferredSource(info)
	lastTS, err := projection.LastStartTS(db, "observations", info.Kind, "")
	if err != nil || lastTS == "" {
		return c, err
	}
	on, err := localDay(lastTS, s.loc)
	if err != nil {
		return c, err
	}
	win, err := projection.LocalWindow(on, "", "", s.loc)
	if err != nil {
		return c, err
	}
	rows, err := projection.ListObservations(db, projection.ObsQuery{Kind: info.Kind, Window: win})
	if err != nil {
		return c, err
	}
	days := compositeDays(rows, "", pref, s.loc, sp.Aggregate)
	if len(days) == 0 {
		c.Source = ""
		return c, nil
	}
	last := days[len(days)-1]
	c.Value, c.Unit = formatAmount(last.Value, last.Unit)
	c.AsOf = last.Day
	c.Source = last.Source
	return c, nil
}

func (s *Server) loadCatalogEpisode(db *sql.DB, info projection.KindInfo, c kindCard) (kindCard, error) {
	lastTS, err := projection.LastStartTS(db, "episodes", info.Kind, "")
	if err != nil || lastTS == "" {
		return c, err
	}
	on, err := localDay(lastTS, s.loc)
	if err != nil {
		return c, err
	}
	c.AsOf = on
	win, err := projection.LocalWindow(on, "", "", s.loc)
	if err != nil {
		return c, err
	}
	rows, err := projection.ListEpisodes(db, projection.EpQuery{Kind: info.Kind, Window: win})
	if err != nil {
		return c, err
	}
	if len(rows) == 0 {
		return c, nil
	}
	if config.IsWorkoutKind(info.Kind) {
		w := parseWorkout(rows[len(rows)-1], nil)
		if w.Duration != "" {
			c.Value = w.Duration
			c.Unit = ""
			return c, nil
		}
		if w.Distance != "" {
			c.Value = w.Distance
			c.Unit = ""
			return c, nil
		}
	}
	c.Value = fmt.Sprintf("%d", len(rows))
	c.Unit = "sessions"
	return c, nil
}

// loadDetailCard is /kind: range lives here. A source lock is only
// honoured when sources actually collide on a local day.
func (s *Server) loadDetailCard(db *sql.DB, info projection.KindInfo, wantSource, rng, wantEnd string) (kindCard, error) {
	lock := sourceLock(info, wantSource)
	c := s.baseCard(info, lock)
	rng = normalizeRange(rng)
	c.Range = rng
	if info.Table == "episodes" {
		c.Source = ""
		return s.loadDetailEpisode(db, info, c, rng, wantEnd)
	}
	sp := config.Lookup(info.Kind)
	pref := preferredSource(info)
	on, first, last, err := s.resolveAnchor(db, "observations", info.Kind, lock, wantEnd)
	if err != nil {
		return c, err
	}
	attachPager(&c, rng, on, first, last, s.loc)
	from, to := rangeDates(on, rng, s.loc)
	to = clipTo(to, last)
	win, err := projection.LocalWindow("", from, to, s.loc)
	if err != nil {
		return c, err
	}
	rows, err := projection.ListObservations(db, projection.ObsQuery{Kind: info.Kind, Source: lock, Window: win})
	if err != nil {
		return c, err
	}
	kept := preferRows(rows, lock, pref, s.loc)
	days := dailySeries(kept, s.loc, sp.Aggregate)
	c.Source = ""
	if len(days) > 0 {
		last := days[len(days)-1]
		c.Value, c.Unit = formatAmount(last.Value, last.Unit)
		c.AsOf = last.Day
		c.Source = last.Source
	}
	c.Spark = sparkSVG(calendarValues(days, on, 14, s.loc))
	pts := observationPlot(kept, from, to, rng, s.loc, sp.Aggregate)
	g := resolveGrain(rng, from, to, firstStart(kept), lastStart(kept), s.loc)
	c.Grain = grainCaption(g, !fromZeroAgg(sp.Aggregate))
	c.ChartJSON = seriesJSON(pts, sp.Display, grainTitle(g))
	c.Rows = formatPlotRows(pts, 20)
	return c, nil
}

func (s *Server) loadDetailEpisode(db *sql.DB, info projection.KindInfo, c kindCard, rng, wantEnd string) (kindCard, error) {
	on, first, last, err := s.resolveAnchor(db, "episodes", info.Kind, "", wantEnd)
	if err != nil {
		return c, err
	}
	attachPager(&c, rng, on, first, last, s.loc)
	from, to := rangeDates(on, rng, s.loc)
	to = clipTo(to, last)
	win, err := projection.LocalWindow("", from, to, s.loc)
	if err != nil {
		return c, err
	}
	rows, err := projection.ListEpisodes(db, projection.EpQuery{Kind: info.Kind, Window: win})
	if err != nil {
		return c, err
	}
	by := map[string]int{}
	var lastEp projection.EpisodeRow
	for _, r := range rows {
		day, err := localDay(r.Start, s.loc)
		if err != nil {
			continue
		}
		by[day]++
		lastEp = r
	}
	if lastEp.Kind != "" {
		day, _ := localDay(lastEp.Start, s.loc)
		c.AsOf = day
		if config.IsWorkoutKind(info.Kind) {
			w := parseWorkout(lastEp, nil)
			if w.Duration != "" {
				c.Value = w.Duration
			} else {
				c.Value = fmt.Sprintf("%d", by[day])
				c.Unit = "sessions"
			}
		} else {
			c.Value = fmt.Sprintf("%d", by[day])
			c.Unit = "sessions"
		}
	}
	var days []dayVal
	for d, n := range by {
		days = append(days, dayVal{Day: d, Value: float64(n), Unit: "sessions"})
	}
	c.Spark = sparkSVG(calendarValues(days, on, 14, s.loc))
	pts := episodePlot(by, from, to, rng, s.loc)
	g := resolveGrain(rng, from, to, dayStartTS(minDay(by), s.loc), dayStartTS(maxDay(by), s.loc), s.loc)
	if g == grainHour {
		g = grainDay
	}
	c.Grain = grainCaption(g, false)
	c.ChartJSON = seriesJSON(pts, c.Display, grainTitle(g))
	if !config.IsWorkoutKind(info.Kind) {
		c.Rows = formatPlotRows(pts, 20)
	}
	return c, nil
}

func (s *Server) listWorkouts(db *sql.DB, kind, rng, wantEnd string) ([]workoutView, error) {
	on, _, last, err := s.resolveAnchor(db, "episodes", kind, "", wantEnd)
	if err != nil {
		return nil, err
	}
	from, to := rangeDates(on, rng, s.loc)
	to = clipTo(to, last)
	win, err := projection.LocalWindow("", from, to, s.loc)
	if err != nil {
		return nil, err
	}
	rows, err := projection.ListEpisodes(db, projection.EpQuery{Kind: kind, Window: win})
	if err != nil {
		return nil, err
	}
	out := make([]workoutView, 0, len(rows))
	for i := len(rows) - 1; i >= 0; i-- {
		out = append(out, parseWorkout(rows[i], func(ts string) string {
			d, _ := localDay(ts, s.loc)
			return d
		}))
	}
	return out, nil
}

func tailValues(days []dayVal, n int) []float64 {
	days = tailDays(days, n)
	out := make([]float64, len(days))
	for i, d := range days {
		out[i] = d.Value
	}
	return out
}

func tailDays(days []dayVal, n int) []dayVal {
	if len(days) > n {
		return days[len(days)-n:]
	}
	return days
}

func formatRows(days []dayVal) []seriesRow {
	out := make([]seriesRow, 0, len(days))
	for i := len(days) - 1; i >= 0; i-- {
		d := days[i]
		v, u := formatAmount(d.Value, d.Unit)
		out = append(out, seriesRow{Day: d.Day, Value: v, Unit: u, Source: d.Source})
	}
	return out
}

func kindQuery(rng, end, source string) string {
	q := url.Values{}
	if rng != "" {
		q.Set("range", rng)
	}
	if end != "" && rng != "all" {
		q.Set("end", end)
	}
	if source != "" {
		q.Set("source", source)
	}
	enc := q.Encode()
	if enc == "" {
		return ""
	}
	return "?" + enc
}

func normalizeRange(rng string) string {
	switch rng {
	case "1d", "7d", "all":
		return rng
	case "1y", "year":
		return "year"
	case "90d":
		return "month"
	default:
		return "month"
	}
}

func lastDayOfMonth(t time.Time, loc *time.Location) time.Time {
	t = time.Date(t.Year(), t.Month(), 1, 0, 0, 0, 0, loc)
	return t.AddDate(0, 1, -1)
}

func sameWindow(a, b, rng string, loc *time.Location) bool {
	ta, err1 := parseDay(a, loc)
	tb, err2 := parseDay(b, loc)
	if err1 != nil || err2 != nil {
		return a == b
	}
	switch normalizeRange(rng) {
	case "month":
		return ta.Year() == tb.Year() && ta.Month() == tb.Month()
	case "year":
		return ta.Year() == tb.Year()
	default:
		return a == b
	}
}

func shiftDay(on, rng string, dir int, loc *time.Location) string {
	t, err := parseDay(on, loc)
	if err != nil {
		return on
	}
	switch normalizeRange(rng) {
	case "month":
		first := time.Date(t.Year(), t.Month(), 1, 0, 0, 0, 0, loc)
		return lastDayOfMonth(first.AddDate(0, dir, 0), loc).Format("2006-01-02")
	case "year":
		return time.Date(t.Year()+dir, 12, 31, 0, 0, 0, 0, loc).Format("2006-01-02")
	case "7d":
		return t.AddDate(0, 0, dir*7).Format("2006-01-02")
	default:
		return t.AddDate(0, 0, dir).Format("2006-01-02")
	}
}

func windowLabel(from, to string, loc *time.Location) string {
	a, err1 := parseDay(from, loc)
	b, err2 := parseDay(to, loc)
	if err1 != nil || err2 != nil {
		if to != "" {
			return to
		}
		return from
	}
	if a.Equal(b) {
		return b.Format("2 January 2006")
	}
	if a.Year() == b.Year() && a.Month() == b.Month() {
		return a.Format("2") + "–" + b.Format("2 January 2006")
	}
	if a.Year() == b.Year() {
		return a.Format("2 January") + "–" + b.Format("2 January 2006")
	}
	return a.Format("2 January 2006") + "–" + b.Format("2 January 2006")
}

func attachPager(c *kindCard, rng, end, first, last string, loc *time.Location) {
	if rng == "all" || end == "" {
		return
	}
	c.End = end
	c.First = first
	c.Last = last
	c.HasPager = true
	from, to := rangeDates(end, rng, loc)
	c.Window = windowCaption(rng, from, clipTo(to, last), end, loc)
	prev := shiftDay(end, rng, -1, loc)
	_, prevTo := rangeDates(prev, rng, loc)
	if first == "" || prevTo >= first {
		c.PrevEnd = prev
	}
	if last != "" && !sameWindow(end, last, rng, loc) {
		next := shiftDay(end, rng, 1, loc)
		if next > last {
			next = last
		}
		c.NextEnd = next
	}
}

func (s *Server) resolveAnchor(db *sql.DB, table, kind, source, wantEnd string) (end, first, last string, err error) {
	// Pager bounds are kind-wide so locking a source cannot shrink or
	// jump the selected day.
	lastTS, err := projection.LastStartTS(db, table, kind, "")
	if err != nil {
		return "", "", "", err
	}
	firstTS, err := projection.FirstStartTS(db, table, kind, "")
	if err != nil {
		return "", "", "", err
	}
	if lastTS != "" {
		last, err = localDay(lastTS, s.loc)
		if err != nil {
			return "", "", "", err
		}
	}
	if firstTS != "" {
		first, err = localDay(firstTS, s.loc)
		if err != nil {
			return "", "", "", err
		}
	}
	if last == "" {
		last, err = projection.LatestLocalDay(db, s.loc)
		if err != nil {
			return "", "", "", err
		}
	}
	end = last
	if wantEnd != "" {
		if t, e := parseDay(wantEnd, s.loc); e == nil {
			end = t.Format("2006-01-02")
		}
	} else if source != "" {
		if srcTS, e := projection.LastStartTS(db, table, kind, source); e == nil && srcTS != "" {
			if d, e := localDay(srcTS, s.loc); e == nil && d != "" {
				end = d
			}
		}
	}
	if last != "" && end > last {
		end = last
	}
	return end, first, last, nil
}

func clipTo(to, last string) string {
	if last != "" && to > last {
		return last
	}
	return to
}

func rangeDates(on, rng string, loc *time.Location) (from, to string) {
	end, err := time.ParseInLocation("2006-01-02", on, loc)
	if err != nil {
		end = time.Now().In(loc)
		on = end.Format("2006-01-02")
	}
	switch normalizeRange(rng) {
	case "1d":
		return on, on
	case "7d":
		return end.AddDate(0, 0, -6).Format("2006-01-02"), on
	case "year":
		from = time.Date(end.Year(), 1, 1, 0, 0, 0, 0, loc).Format("2006-01-02")
		to = time.Date(end.Year(), 12, 31, 0, 0, 0, 0, loc).Format("2006-01-02")
		return from, to
	case "all":
		return "", ""
	default: // month
		from = time.Date(end.Year(), end.Month(), 1, 0, 0, 0, 0, loc).Format("2006-01-02")
		to = lastDayOfMonth(end, loc).Format("2006-01-02")
		return from, to
	}
}

func windowCaption(rng, from, to, end string, loc *time.Location) string {
	t, err := parseDay(end, loc)
	if err != nil {
		t, err = parseDay(to, loc)
	}
	switch normalizeRange(rng) {
	case "month":
		if err != nil {
			return to
		}
		return t.Format("January 2006")
	case "year":
		if err != nil {
			return to
		}
		return t.Format("2006")
	default:
		return windowLabel(from, to, loc)
	}
}

func sparkSVG(vals []float64) template.HTML {
	if len(vals) == 0 {
		return ""
	}
	const w, h, top = 300.0, 40.0, 2.0
	min, max := math.NaN(), math.NaN()
	any := false
	for _, v := range vals {
		if math.IsNaN(v) {
			continue
		}
		if !any {
			min, max, any = v, v, true
			continue
		}
		if v < min {
			min = v
		}
		if v > max {
			max = v
		}
	}
	if !any {
		return ""
	}
	span := max - min
	if span == 0 {
		span = 1
	}
	n := float64(len(vals))
	slot := w / n
	barW := slot * 0.67
	if barW > 14 {
		barW = 14
	}
	usable := h - top
	var b strings.Builder
	b.WriteString(`<svg class="spark-bars" viewBox="0 0 300 40" width="300" height="40" aria-hidden="true">`)
	for i, v := range vals {
		if math.IsNaN(v) {
			continue
		}
		bh := 3 + (v-min)/span*(usable-3)
		x := slot*float64(i) + (slot-barW)/2
		y := h - bh
		cls := ""
		if i == len(vals)-1 {
			cls = ` class="last"`
		}
		fmt.Fprintf(&b, `<rect x="%.1f" y="%.1f" width="%.1f" height="%.1f" rx="2"%s/>`, x, y, barW, bh, cls)
	}
	b.WriteString(`</svg>`)
	return template.HTML(b.String())
}

func seriesJSON(pts []plotPoint, label, xTitle string) template.JS {
	labels := make([]string, len(pts))
	data := make([]any, len(pts))
	center := make([]any, len(pts))
	yTitle := ""
	fromZero := true
	rangeBars := false
	for i, p := range pts {
		labels[i] = p.Label
		if p.Value == nil {
			continue
		}
		v, u := displayAmount(*p.Value, p.Unit)
		if yTitle == "" {
			yTitle = u
		}
		if p.FromZero {
			data[i] = v
			center[i] = v
			continue
		}
		fromZero = false
		rangeBars = true
		lo, hi := v, v
		if p.Lo != nil {
			lo, _ = displayAmount(*p.Lo, p.Unit)
		}
		if p.Hi != nil {
			hi, _ = displayAmount(*p.Hi, p.Unit)
		}
		data[i] = []float64{lo, hi}
		center[i] = v
	}
	if xTitle == "" {
		xTitle = "Day"
	}
	ds := chartDS{Label: label, Data: data}
	if rangeBars {
		ds.Center = center
	}
	return marshalChart(chartCfg{
		Type:        "bar",
		Labels:      labels,
		Datasets:    []chartDS{ds},
		XTitle:      xTitle,
		YTitle:      yTitle,
		BeginAtZero: fromZero,
		RangeBars:   rangeBars,
	})
}

func formatPlotRows(pts []plotPoint, n int) []seriesRow {
	var filled []plotPoint
	for _, p := range pts {
		if p.Value != nil {
			filled = append(filled, p)
		}
	}
	if len(filled) > n {
		filled = filled[len(filled)-n:]
	}
	out := make([]seriesRow, 0, len(filled))
	for i := len(filled) - 1; i >= 0; i-- {
		p := filled[i]
		v, u := formatAmount(*p.Value, p.Unit)
		if !p.FromZero && p.Lo != nil && p.Hi != nil && *p.Lo != *p.Hi {
			lo, _ := formatAmount(*p.Lo, p.Unit)
			hi, _ := formatAmount(*p.Hi, p.Unit)
			v = v + " · " + lo + "–" + hi
		}
		out = append(out, seriesRow{Day: p.Label, Value: v, Unit: u, Source: p.Source})
	}
	return out
}

func calendarValues(days []dayVal, on string, n int, loc *time.Location) []float64 {
	if on == "" || n <= 0 {
		return tailValues(days, n)
	}
	end, err := parseDay(on, loc)
	if err != nil {
		return tailValues(days, n)
	}
	by := map[string]float64{}
	for _, d := range days {
		by[d.Day] = d.Value
	}
	out := make([]float64, n)
	start := end.AddDate(0, 0, -(n - 1))
	for i := 0; i < n; i++ {
		key := start.AddDate(0, 0, i).Format("2006-01-02")
		if v, ok := by[key]; ok {
			out[i] = v
		} else {
			out[i] = math.NaN()
		}
	}
	return out
}
