package ui

import (
	"time"

	"archive/internal/projection"
)

type grain int

const (
	grainHour grain = iota
	grainDay
	grainWeek
	grainMonth
)

type plotPoint struct {
	Label    string
	Value    *float64
	Lo, Hi   *float64
	Unit     string
	Source   string
	FromZero bool
}

func grainOf(rng string, from, to time.Time) grain {
	switch normalizeRange(rng) {
	case "1d", "7d":
		return grainHour
	case "year":
		return grainMonth
	case "all":
		days := 0.0
		if !to.Before(from) {
			days = to.Sub(from).Hours() / 24
		}
		switch {
		case days <= 8:
			return grainHour
		case days <= 35:
			return grainDay
		case days <= 100:
			return grainWeek
		default:
			return grainMonth
		}
	default:
		return grainDay
	}
}

func grainTitle(g grain) string {
	switch g {
	case grainHour:
		return "Hour"
	case grainWeek:
		return "Week"
	case grainMonth:
		return "Month"
	default:
		return "Day"
	}
}

func grainCaption(g grain, rangeBars bool) string {
	unit := "day"
	switch g {
	case grainHour:
		unit = "hour"
	case grainWeek:
		unit = "week"
	case grainMonth:
		unit = "month"
	}
	if rangeBars {
		return "One bar per " + unit + " (min–max). Empty " + unit + "s stay blank."
	}
	return "One bar per " + unit + ". Empty " + unit + "s stay blank."
}

func fromZeroAgg(agg string) bool {
	return agg == "sum" || agg == "count"
}

func dateOnly(t time.Time, loc *time.Location) time.Time {
	t = t.In(loc)
	return time.Date(t.Year(), t.Month(), t.Day(), 0, 0, 0, 0, loc)
}

func mondayOf(t time.Time, loc *time.Location) time.Time {
	t = dateOnly(t, loc)
	wd := t.Weekday()
	if wd == time.Sunday {
		return t.AddDate(0, 0, -6)
	}
	return t.AddDate(0, 0, -int(wd-time.Monday))
}

func monthStart(t time.Time, loc *time.Location) time.Time {
	t = t.In(loc)
	return time.Date(t.Year(), t.Month(), 1, 0, 0, 0, 0, loc)
}

func truncateGrain(t time.Time, g grain, loc *time.Location) time.Time {
	t = t.In(loc)
	switch g {
	case grainHour:
		return time.Date(t.Year(), t.Month(), t.Day(), t.Hour(), 0, 0, 0, loc)
	case grainWeek:
		return mondayOf(t, loc)
	case grainMonth:
		return monthStart(t, loc)
	default:
		return dateOnly(t, loc)
	}
}

func grainLabel(t time.Time, g grain) string {
	switch g {
	case grainHour:
		return t.Format("2 Jan 15:00")
	case grainWeek:
		end := t.AddDate(0, 0, 6)
		if t.Month() == end.Month() {
			return t.Format("2") + "–" + end.Format("2 Jan")
		}
		return t.Format("2 Jan") + "–" + end.Format("2 Jan")
	case grainMonth:
		return t.Format("Jan 2006")
	default:
		return t.Format("2 Jan")
	}
}

func parseDay(s string, loc *time.Location) (time.Time, error) {
	return time.ParseInLocation("2006-01-02", s, loc)
}

func plotBounds(from, to string, first, last string, loc *time.Location) (time.Time, time.Time, bool) {
	var start, end time.Time
	var err error
	if from != "" {
		start, err = parseDay(from, loc)
	} else if first != "" {
		t, e := time.Parse(time.RFC3339Nano, first)
		if e != nil {
			return time.Time{}, time.Time{}, false
		}
		start = dateOnly(t, loc)
	} else {
		return time.Time{}, time.Time{}, false
	}
	if err != nil {
		return time.Time{}, time.Time{}, false
	}
	if to != "" {
		end, err = parseDay(to, loc)
	} else if last != "" {
		t, e := time.Parse(time.RFC3339Nano, last)
		if e != nil {
			return time.Time{}, time.Time{}, false
		}
		end = dateOnly(t, loc)
	} else {
		end = start
	}
	if err != nil {
		return time.Time{}, time.Time{}, false
	}
	if end.Before(start) {
		start, end = end, start
	}
	return start, end, true
}

func eachBucket(from, to time.Time, g grain, loc *time.Location) []time.Time {
	switch g {
	case grainHour:
		var out []time.Time
		for d := dateOnly(from, loc); !d.After(dateOnly(to, loc)); d = d.AddDate(0, 0, 1) {
			for h := 0; h < 24; h++ {
				out = append(out, time.Date(d.Year(), d.Month(), d.Day(), h, 0, 0, 0, loc))
			}
		}
		return out
	case grainWeek:
		var out []time.Time
		for t := mondayOf(from, loc); !t.After(mondayOf(to, loc)); t = t.AddDate(0, 0, 7) {
			out = append(out, t)
		}
		return out
	case grainMonth:
		var out []time.Time
		for t := monthStart(from, loc); !t.After(monthStart(to, loc)); t = t.AddDate(0, 1, 0) {
			out = append(out, t)
		}
		return out
	default:
		var out []time.Time
		for t := dateOnly(from, loc); !t.After(dateOnly(to, loc)); t = t.AddDate(0, 0, 1) {
			out = append(out, t)
		}
		return out
	}
}

func bucketKey(t time.Time, g grain, loc *time.Location) string {
	t = truncateGrain(t, g, loc)
	switch g {
	case grainHour:
		return t.Format("2006-01-02T15")
	case grainMonth:
		return t.Format("2006-01")
	default:
		return t.Format("2006-01-02")
	}
}

type acc struct {
	vals   []float64
	n      int
	unit   string
	source string
}

func rollupAgg(agg string) string {
	if agg == "count" {
		return "sum"
	}
	return agg
}

func reduceBucket(a *acc, agg string, fromZero bool) plotPoint {
	p := plotPoint{Unit: a.unit, Source: a.source, FromZero: fromZero}
	if a == nil {
		return p
	}
	if agg == "count" {
		if a.n == 0 {
			return p
		}
		v := float64(a.n)
		p.Value = &v
		if len(a.vals) > 0 {
			lo, hi := sliceMin(a.vals), sliceMax(a.vals)
			p.Lo, p.Hi = &lo, &hi
		}
		return p
	}
	if len(a.vals) == 0 {
		return p
	}
	var v float64
	switch agg {
	case "sum":
		v = reduce(a.vals, "sum")
	case "max":
		v = sliceMax(a.vals)
	case "min":
		v = sliceMin(a.vals)
	case "latest":
		v = a.vals[len(a.vals)-1]
	default:
		v = reduce(a.vals, "avg")
	}
	p.Value = &v
	lo, hi := sliceMin(a.vals), sliceMax(a.vals)
	p.Lo, p.Hi = &lo, &hi
	return p
}

func fillPlot(buckets []time.Time, by map[string]*acc, g grain, loc *time.Location, agg string, fromZero bool) []plotPoint {
	out := make([]plotPoint, 0, len(buckets))
	for _, t := range buckets {
		p := plotPoint{Label: grainLabel(t, g), FromZero: fromZero}
		if a := by[bucketKey(t, g, loc)]; a != nil {
			p = reduceBucket(a, agg, fromZero)
			p.Label = grainLabel(t, g)
		}
		out = append(out, p)
	}
	return out
}

func samplePlot(rows []projection.ObservationRow, from, to time.Time, g grain, loc *time.Location, agg string) []plotPoint {
	buckets := eachBucket(from, to, g, loc)
	by := map[string]*acc{}
	for _, r := range rows {
		t, err := time.Parse(time.RFC3339Nano, r.Start)
		if err != nil {
			continue
		}
		k := bucketKey(t, g, loc)
		a := by[k]
		if a == nil {
			a = &acc{unit: r.Unit, source: r.Source}
			by[k] = a
		}
		a.n++
		if v, ok := parseNum(r.Value); ok {
			a.vals = append(a.vals, v)
		}
		if r.Unit != "" {
			a.unit = r.Unit
		}
		if r.Source != "" {
			a.source = r.Source
		}
	}
	return fillPlot(buckets, by, g, loc, agg, fromZeroAgg(agg))
}

func dayPlot(days []dayVal, from, to time.Time, g grain, loc *time.Location, agg string) []plotPoint {
	buckets := eachBucket(from, to, g, loc)
	by := map[string]*acc{}
	for _, d := range days {
		t, err := parseDay(d.Day, loc)
		if err != nil {
			continue
		}
		k := bucketKey(t, g, loc)
		a := by[k]
		if a == nil {
			a = &acc{unit: d.Unit, source: d.Source}
			by[k] = a
		}
		a.n++
		a.vals = append(a.vals, d.Value)
		if d.Unit != "" {
			a.unit = d.Unit
		}
		if d.Source != "" {
			a.source = d.Source
		}
	}
	return fillPlot(buckets, by, g, loc, rollupAgg(agg), fromZeroAgg(agg))
}

func observationPlot(rows []projection.ObservationRow, from, to, rng string, loc *time.Location, agg string) []plotPoint {
	if loc == nil {
		loc = time.UTC
	}
	first, last := "", ""
	if len(rows) > 0 {
		first, last = rows[0].Start, rows[len(rows)-1].Start
	}
	start, end, ok := plotBounds(from, to, first, last, loc)
	if !ok {
		return nil
	}
	g := grainOf(rng, start, end)
	return samplePlot(rows, start, end, g, loc, agg)
}

func episodePlot(byDay map[string]int, from, to, rng string, loc *time.Location) []plotPoint {
	if loc == nil {
		loc = time.UTC
	}
	var days []dayVal
	var first, last string
	for d, n := range byDay {
		days = append(days, dayVal{Day: d, Value: float64(n), Unit: "sessions"})
		if first == "" || d < first {
			first = d
		}
		if last == "" || d > last {
			last = d
		}
	}
	start, end, ok := plotBounds(from, to, dayStartTS(first, loc), dayStartTS(last, loc), loc)
	if !ok {
		return nil
	}
	g := grainOf(rng, start, end)
	if g == grainHour {
		g = grainDay
	}
	return dayPlot(days, start, end, g, loc, "sum")
}

func dayStartTS(day string, loc *time.Location) string {
	t, err := parseDay(day, loc)
	if err != nil {
		return ""
	}
	return t.UTC().Format(time.RFC3339Nano)
}

func firstStart(rows []projection.ObservationRow) string {
	if len(rows) == 0 {
		return ""
	}
	return rows[0].Start
}

func lastStart(rows []projection.ObservationRow) string {
	if len(rows) == 0 {
		return ""
	}
	return rows[len(rows)-1].Start
}

func minDay(by map[string]int) string {
	var m string
	for d := range by {
		if m == "" || d < m {
			m = d
		}
	}
	return m
}

func maxDay(by map[string]int) string {
	var m string
	for d := range by {
		if m == "" || d > m {
			m = d
		}
	}
	return m
}

func resolveGrain(rng, from, to, first, last string, loc *time.Location) grain {
	start, end, ok := plotBounds(from, to, first, last, loc)
	if !ok {
		return grainOf(rng, time.Time{}, time.Time{})
	}
	return grainOf(rng, start, end)
}
