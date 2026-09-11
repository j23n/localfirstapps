package projection

import (
	"database/sql"
	"fmt"
	"sort"
	"time"
)

// GapReport is the local calendar days between first and last data
// that have no observation (or episode) start in that day.
type GapReport struct {
	Kind    string   `json:"kind,omitempty"` // empty = overall
	Table   string   `json:"table,omitempty"`
	First   string   `json:"first,omitempty"` // YYYY-MM-DD local
	Last    string   `json:"last,omitempty"`
	Present []string `json:"present,omitempty"`
	Missing []string `json:"missing,omitempty"`
}

// PresentN is how many local days have at least one row.
func (g GapReport) PresentN() int { return len(g.Present) }

// MissingN is how many local days in [First, Last] have no row.
func (g GapReport) MissingN() int { return len(g.Missing) }

// SpanDays is inclusive calendar days from first to last, or 0 if empty.
func (g GapReport) SpanDays() int {
	if g.First == "" || g.Last == "" {
		return 0
	}
	a, err1 := time.Parse("2006-01-02", g.First)
	b, err2 := time.Parse("2006-01-02", g.Last)
	if err1 != nil || err2 != nil {
		return 0
	}
	return int(b.Sub(a).Hours()/24) + 1
}

// ObservationGaps lists missing local days for one observation kind
// between its first and last sample. Empty kind means every observation.
func ObservationGaps(db *sql.DB, kind string, loc *time.Location) (GapReport, error) {
	if loc == nil {
		loc = time.UTC
	}
	q := `SELECT start_ts FROM observations`
	var args []any
	if kind != "" {
		q += ` WHERE kind = ?`
		args = append(args, kind)
	}
	q += ` ORDER BY start_ts`
	return scanGaps(db, q, args, kind, "observations", loc)
}

// EpisodeGaps is the same scan over episodes.
func EpisodeGaps(db *sql.DB, kind string, loc *time.Location) (GapReport, error) {
	if loc == nil {
		loc = time.UTC
	}
	q := `SELECT start_ts FROM episodes`
	var args []any
	if kind != "" {
		q += ` WHERE kind = ?`
		args = append(args, kind)
	}
	q += ` ORDER BY start_ts`
	return scanGaps(db, q, args, kind, "episodes", loc)
}

// OverallGaps is days with no observation of any kind, plus no episode.
func OverallGaps(db *sql.DB, loc *time.Location) (GapReport, error) {
	if loc == nil {
		loc = time.UTC
	}
	obs, err := ObservationGaps(db, "", loc)
	if err != nil {
		return GapReport{}, err
	}
	eps, err := EpisodeGaps(db, "", loc)
	if err != nil {
		return GapReport{}, err
	}
	set := map[string]bool{}
	for _, d := range obs.Present {
		set[d] = true
	}
	for _, d := range eps.Present {
		set[d] = true
	}
	if len(set) == 0 {
		return GapReport{Table: "overall"}, nil
	}
	present := make([]string, 0, len(set))
	for d := range set {
		present = append(present, d)
	}
	sort.Strings(present)
	g := GapReport{
		Table:   "overall",
		First:   present[0],
		Last:    present[len(present)-1],
		Present: present,
		Missing: missingBetween(present[0], present[len(present)-1], set),
	}
	return g, nil
}

func scanGaps(db *sql.DB, q string, args []any, kind, table string, loc *time.Location) (GapReport, error) {
	rows, err := db.Query(q, args...)
	if err != nil {
		return GapReport{}, err
	}
	defer rows.Close()
	set := map[string]bool{}
	for rows.Next() {
		var ts string
		if err := rows.Scan(&ts); err != nil {
			return GapReport{}, err
		}
		day, err := localDay(ts, loc)
		if err != nil {
			return GapReport{}, err
		}
		set[day] = true
	}
	if err := rows.Err(); err != nil {
		return GapReport{}, err
	}
	g := GapReport{Kind: kind, Table: table}
	if len(set) == 0 {
		return g, nil
	}
	present := make([]string, 0, len(set))
	for d := range set {
		present = append(present, d)
	}
	sort.Strings(present)
	g.First = present[0]
	g.Last = present[len(present)-1]
	g.Present = present
	g.Missing = missingBetween(g.First, g.Last, set)
	return g, nil
}

func localDay(ts string, loc *time.Location) (string, error) {
	t, err := time.Parse(time.RFC3339Nano, ts)
	if err != nil {
		return "", fmt.Errorf("start_ts %q: %w", ts, err)
	}
	return t.In(loc).Format("2006-01-02"), nil
}

func missingBetween(first, last string, present map[string]bool) []string {
	a, err1 := time.Parse("2006-01-02", first)
	b, err2 := time.Parse("2006-01-02", last)
	if err1 != nil || err2 != nil {
		return nil
	}
	var out []string
	for d := a; !d.After(b); d = d.AddDate(0, 0, 1) {
		s := d.Format("2006-01-02")
		if !present[s] {
			out = append(out, s)
		}
	}
	return out
}

// LatestLocalDay is the most recent local calendar day that has any observation.
func LatestLocalDay(db *sql.DB, loc *time.Location) (string, error) {
	if loc == nil {
		loc = time.UTC
	}
	var ts sql.NullString
	if err := db.QueryRow(`SELECT MAX(start_ts) FROM observations`).Scan(&ts); err != nil {
		return "", err
	}
	if !ts.Valid || ts.String == "" {
		return "", nil
	}
	return localDay(ts.String, loc)
}
