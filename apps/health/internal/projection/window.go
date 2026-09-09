package projection

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

// DisplayLocation is the zone used to interpret -on/-from/-to calendar dates.
// ARCHIVE_TZ may be an IANA name (Europe/Berlin) or a fixed offset (+0200).
// Unset means time.Local.
func DisplayLocation() (*time.Location, error) {
	return ParseLocation(os.Getenv("ARCHIVE_TZ"))
}

// ParseLocation accepts IANA names or numeric offsets (+0200, +02:00, -0800).
func ParseLocation(name string) (*time.Location, error) {
	name = strings.TrimSpace(name)
	if name == "" {
		return time.Local, nil
	}
	if loc, err := time.LoadLocation(name); err == nil {
		return loc, nil
	}
	off, err := parseOffset(name)
	if err != nil {
		return nil, fmt.Errorf("timezone %q: want IANA name or offset like +0200", name)
	}
	return time.FixedZone(name, off), nil
}

func parseOffset(s string) (int, error) {
	s = strings.TrimSpace(s)
	if s == "Z" || s == "UTC" || s == "GMT" {
		return 0, nil
	}
	if len(s) < 3 || (s[0] != '+' && s[0] != '-') {
		return 0, fmt.Errorf("bad offset")
	}
	sign := 1
	if s[0] == '-' {
		sign = -1
	}
	rest := s[1:]
	rest = strings.ReplaceAll(rest, ":", "")
	if len(rest) != 4 {
		return 0, fmt.Errorf("bad offset")
	}
	h, err := strconv.Atoi(rest[:2])
	if err != nil || h > 23 {
		return 0, fmt.Errorf("bad offset")
	}
	m, err := strconv.Atoi(rest[2:])
	if err != nil || m > 59 {
		return 0, fmt.Errorf("bad offset")
	}
	return sign * (h*3600 + m*60), nil
}

// TimeWindow is a half-open UTC interval [FromUTC, ToUTC) derived from local dates.
type TimeWindow struct {
	FromUTC time.Time
	ToUTC   time.Time
	Offset  string // offset in effect at FromUTC, e.g. "+0200"
	Label   string
}

// Empty reports whether no bound is set.
func (w TimeWindow) Empty() bool {
	return w.FromUTC.IsZero() && w.ToUTC.IsZero()
}

// LocalWindow converts calendar dates in loc into a UTC window.
// on is a single local day. from/to are inclusive local days.
// on cannot be combined with from/to.
func LocalWindow(on, from, to string, loc *time.Location) (TimeWindow, error) {
	if loc == nil {
		loc = time.UTC
	}
	if on != "" && (from != "" || to != "") {
		return TimeWindow{}, fmt.Errorf("use -on or -from/-to, not both")
	}
	if on != "" {
		start, err := parseLocalDate(on, loc)
		if err != nil {
			return TimeWindow{}, err
		}
		end := start.AddDate(0, 0, 1)
		return window(start, end, on), nil
	}
	var start, end time.Time
	if from != "" {
		var err error
		start, err = parseLocalDate(from, loc)
		if err != nil {
			return TimeWindow{}, err
		}
	}
	if to != "" {
		t, err := parseLocalDate(to, loc)
		if err != nil {
			return TimeWindow{}, err
		}
		end = t.AddDate(0, 0, 1)
	}
	label := from
	if to != "" && to != from {
		if label != "" {
			label += ".." + to
		} else {
			label = ".." + to
		}
	}
	if from == "" && to == "" {
		return TimeWindow{}, nil
	}
	return window(start, end, label), nil
}

func parseLocalDate(s string, loc *time.Location) (time.Time, error) {
	t, err := time.ParseInLocation("2006-01-02", s, loc)
	if err != nil {
		return time.Time{}, fmt.Errorf("date %q: want YYYY-MM-DD", s)
	}
	return t, nil
}

func window(start, end time.Time, label string) TimeWindow {
	w := TimeWindow{Label: label}
	if !start.IsZero() {
		w.FromUTC = start.UTC()
		w.Offset = start.Format("-0700")
	}
	if !end.IsZero() {
		w.ToUTC = end.UTC()
		if w.Offset == "" {
			w.Offset = end.Add(-time.Second).Format("-0700")
		}
	}
	return w
}

// Note describes the local→UTC conversion for humans and tests.
func (w TimeWindow) Note() string {
	if w.Empty() {
		return ""
	}
	var b strings.Builder
	b.WriteString("local ")
	b.WriteString(w.Label)
	if w.Offset != "" {
		b.WriteString(" offset ")
		b.WriteString(w.Offset)
	}
	b.WriteString(" → UTC ")
	if !w.FromUTC.IsZero() {
		b.WriteString(w.FromUTC.Format(time.RFC3339))
	} else {
		b.WriteString("-inf")
	}
	b.WriteString(" .. ")
	if !w.ToUTC.IsZero() {
		b.WriteString(w.ToUTC.Format(time.RFC3339))
	} else {
		b.WriteString("+inf")
	}
	return b.String()
}
