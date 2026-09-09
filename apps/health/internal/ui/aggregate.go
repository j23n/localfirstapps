package ui

import (
	"sort"
	"time"

	"archive/config"
	"archive/internal/projection"
)

type dayVal struct {
	Day    string
	Value  float64
	Lo, Hi float64
	Unit   string
	N      int
	Source string
}

// preferRows keeps one source per local day so dailySeries never sums
// Watch and phone. If lock is set, only that source is kept. Otherwise
// the preferred source wins on a collision; a day with only the other
// source is kept.
func preferRows(rows []projection.ObservationRow, lock, preferred string, loc *time.Location) []projection.ObservationRow {
	if loc == nil {
		loc = time.UTC
	}
	if lock != "" {
		out := make([]projection.ObservationRow, 0, len(rows))
		for _, r := range rows {
			if r.Source == lock {
				out = append(out, r)
			}
		}
		return out
	}
	byDay := map[string]map[string][]projection.ObservationRow{}
	var order []string
	for _, r := range rows {
		day, err := localDay(r.Start, loc)
		if err != nil {
			continue
		}
		if byDay[day] == nil {
			byDay[day] = map[string][]projection.ObservationRow{}
			order = append(order, day)
		}
		byDay[day][r.Source] = append(byDay[day][r.Source], r)
	}
	var out []projection.ObservationRow
	for _, day := range order {
		srcs := byDay[day]
		if preferred != "" {
			if rs, ok := srcs[preferred]; ok {
				out = append(out, rs...)
				continue
			}
		}
		if len(srcs) == 1 {
			for _, rs := range srcs {
				out = append(out, rs...)
			}
			continue
		}
		names := make([]string, 0, len(srcs))
		for s := range srcs {
			names = append(names, s)
		}
		sort.Strings(names)
		out = append(out, srcs[names[0]]...)
	}
	return out
}

func compositeDays(rows []projection.ObservationRow, lock, preferred string, loc *time.Location, agg string) []dayVal {
	return dailySeries(preferRows(rows, lock, preferred, loc), loc, agg)
}

// dailySeries groups rows by local day using the kind's configured aggregate.
// Caller must already have run preferRows so a day never mixes sources.
func dailySeries(rows []projection.ObservationRow, loc *time.Location, agg string) []dayVal {
	if loc == nil {
		loc = time.UTC
	}
	type bucket struct {
		vals   []float64
		unit   string
		n      int
		source string
	}
	by := map[string]*bucket{}
	var order []string
	for _, r := range rows {
		day, err := localDay(r.Start, loc)
		if err != nil {
			continue
		}
		b := by[day]
		if b == nil {
			b = &bucket{unit: r.Unit, source: r.Source}
			by[day] = b
			order = append(order, day)
		}
		b.n++
		if v, ok := parseNum(r.Value); ok {
			b.vals = append(b.vals, v)
		}
		if r.Unit != "" {
			b.unit = r.Unit
		}
	}
	sort.Strings(order)
	out := make([]dayVal, 0, len(order))
	for _, day := range order {
		b := by[day]
		dv := dayVal{Day: day, Unit: b.unit, N: b.n, Source: b.source}
		switch agg {
		case "count":
			dv.Value = float64(b.n)
		case "sum":
			if len(b.vals) == 0 {
				continue
			}
			dv.Value = reduce(b.vals, "sum")
		case "max":
			if len(b.vals) == 0 {
				continue
			}
			dv.Value = sliceMax(b.vals)
		case "min":
			if len(b.vals) == 0 {
				continue
			}
			dv.Value = sliceMin(b.vals)
		case "latest":
			if len(b.vals) == 0 {
				continue
			}
			dv.Value = b.vals[len(b.vals)-1]
		default: // mean
			if len(b.vals) == 0 {
				continue
			}
			dv.Value = reduce(b.vals, "avg")
		}
		if len(b.vals) > 0 {
			dv.Lo = sliceMin(b.vals)
			dv.Hi = sliceMax(b.vals)
		} else {
			dv.Lo, dv.Hi = dv.Value, dv.Value
		}
		out = append(out, dv)
	}
	return out
}

func sliceMax(v []float64) float64 {
	m := v[0]
	for _, x := range v[1:] {
		if x > m {
			m = x
		}
	}
	return m
}

func sliceMin(v []float64) float64 {
	m := v[0]
	for _, x := range v[1:] {
		if x < m {
			m = x
		}
	}
	return m
}

// Percentile is 100 * (count of hist values strictly below x) / len(hist).
func Percentile(x float64, hist []float64) float64 {
	if len(hist) == 0 {
		return 0
	}
	var below int
	for _, h := range hist {
		if h < x {
			below++
		}
	}
	return 100 * float64(below) / float64(len(hist))
}

const baselineMinDays = 30

// BaselineNote is empty unless there are at least 30 baseline days.
// Language is neutral: unusual for you, never abnormal/high/low.
func BaselineNote(today float64, hist []float64) string {
	if len(hist) < baselineMinDays {
		return ""
	}
	p := Percentile(today, hist)
	if p < 10 {
		return "Lower than usual for you"
	}
	if p > 90 {
		return "Higher than usual for you"
	}
	return ""
}

func baselineWindow(on string, loc *time.Location) (from, to string) {
	t, err := time.ParseInLocation("2006-01-02", on, loc)
	if err != nil {
		return "", ""
	}
	// trailing 90 days excluding today: [on-90, on)
	return t.AddDate(0, 0, -90).Format("2006-01-02"), t.AddDate(0, 0, -1).Format("2006-01-02")
}

func preferredSource(info projection.KindInfo) string {
	if info.Preferred != "" {
		return info.Preferred
	}
	if len(info.Sources) > 0 {
		return info.Sources[0]
	}
	return ""
}

func specFor(kind string) config.Spec {
	return config.Lookup(kind)
}
