package projection

import (
	"database/sql"
	"fmt"
	"sort"
	"strconv"
	"strings"
)

// ObsQuery filters projected observations.
type ObsQuery struct {
	Kind   string
	Source string
	Window TimeWindow
}

// EpQuery filters projected episodes.
type EpQuery struct {
	Kind   string
	Window TimeWindow
}

// ListObservations returns rows matching q, ordered by (start_ts, dedup_key, n).
func ListObservations(db *sql.DB, q ObsQuery) ([]ObservationRow, error) {
	sqlStr := `SELECT dedup_key, n, kind, IFNULL(source,''), IFNULL(source_version,''), IFNULL(device,''), start_ts, end_ts, IFNULL(start_offset,''), IFNULL(end_offset,''), IFNULL(unit,''), IFNULL(value,''), IFNULL(metadata,'{}'), IFNULL(blob_sha256,''), IFNULL(event_id,'')
		FROM observations WHERE 1=1`
	var args []any
	if q.Kind != "" {
		sqlStr += ` AND kind = ?`
		args = append(args, q.Kind)
	}
	if q.Source != "" {
		sqlStr += ` AND source = ?`
		args = append(args, q.Source)
	}
	sqlStr, args = appendWindow(sqlStr, args, q.Window)
	sqlStr += ` ORDER BY start_ts, dedup_key, n`
	rows, err := db.Query(sqlStr, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []ObservationRow
	for rows.Next() {
		var o ObservationRow
		if err := rows.Scan(&o.DedupKey, &o.N, &o.Kind, &o.Source, &o.SourceVersion, &o.Device,
			&o.Start, &o.End, &o.StartOffset, &o.EndOffset, &o.Unit, &o.Value, &o.Metadata, &o.BlobSHA256, &o.EventID); err != nil {
			return nil, err
		}
		out = append(out, o)
	}
	return out, rows.Err()
}

// LastStartTS is the latest start_ts for kind in table (observations or episodes).
// An empty source matches any source. sql.ErrNoRows becomes "".
func LastStartTS(db *sql.DB, table, kind, source string) (string, error) {
	if table != "observations" && table != "episodes" {
		return "", fmt.Errorf("last start: unknown table %s", table)
	}
	q := `SELECT start_ts FROM ` + table + ` WHERE kind = ?`
	args := []any{kind}
	if source != "" {
		q += ` AND source = ?`
		args = append(args, source)
	}
	q += ` ORDER BY start_ts DESC LIMIT 1`
	var ts string
	err := db.QueryRow(q, args...).Scan(&ts)
	if err == sql.ErrNoRows {
		return "", nil
	}
	return ts, err
}

// FirstStartTS is the earliest start_ts for kind in table. Empty source matches any.
func FirstStartTS(db *sql.DB, table, kind, source string) (string, error) {
	if table != "observations" && table != "episodes" {
		return "", fmt.Errorf("first start: unknown table %s", table)
	}
	q := `SELECT start_ts FROM ` + table + ` WHERE kind = ?`
	args := []any{kind}
	if source != "" {
		q += ` AND source = ?`
		args = append(args, source)
	}
	q += ` ORDER BY start_ts ASC LIMIT 1`
	var ts string
	err := db.QueryRow(q, args...).Scan(&ts)
	if err == sql.ErrNoRows {
		return "", nil
	}
	return ts, err
}

// EpisodeByKey loads one episode by dedup_key, including route columns.
func EpisodeByKey(db *sql.DB, key string) (EpisodeRow, error) {
	var e EpisodeRow
	if key == "" {
		return e, sql.ErrNoRows
	}
	var poly []byte
	var segs, pts, moving, elapsed, pauses sql.NullInt64
	var bbox sql.NullString
	var ascent, descent sql.NullFloat64
	q := `SELECT dedup_key, kind, IFNULL(source,''), start_ts, end_ts, IFNULL(start_offset,''), IFNULL(end_offset,''), IFNULL(body,'{}'), IFNULL(blob_sha256,''), IFNULL(event_id,''),
		route_polyline, route_segments, route_points, route_bbox, ascent_m, descent_m, moving_seconds, elapsed_seconds, pause_count
		FROM episodes WHERE dedup_key = ?`
	err := db.QueryRow(q, key).Scan(&e.DedupKey, &e.Kind, &e.Source, &e.Start, &e.End, &e.StartOffset, &e.EndOffset, &e.Body, &e.BlobSHA256, &e.EventID,
		&poly, &segs, &pts, &bbox, &ascent, &descent, &moving, &elapsed, &pauses)
	if err != nil {
		return e, err
	}
	e.RoutePolyline = poly
	if segs.Valid {
		e.RouteSegments = int(segs.Int64)
	}
	if pts.Valid {
		e.RoutePoints = int(pts.Int64)
	}
	if bbox.Valid {
		e.RouteBBox = bbox.String
	}
	if ascent.Valid {
		v := ascent.Float64
		e.AscentM = &v
	}
	if descent.Valid {
		v := descent.Float64
		e.DescentM = &v
	}
	if moving.Valid {
		v := int(moving.Int64)
		e.MovingSeconds = &v
	}
	if elapsed.Valid {
		v := int(elapsed.Int64)
		e.ElapsedSeconds = &v
	}
	if pauses.Valid {
		v := int(pauses.Int64)
		e.PauseCount = &v
	}
	return e, nil
}

// ListWorkoutEvents returns projected WorkoutEvent rows for an episode, in seq order.
func ListWorkoutEvents(db *sql.DB, episodeID string) ([]WorkoutEventRow, error) {
	rows, err := db.Query(`SELECT kind, time_utc FROM workout_event WHERE episode_id = ? ORDER BY seq`, episodeID)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []WorkoutEventRow
	for rows.Next() {
		var e WorkoutEventRow
		if err := rows.Scan(&e.Kind, &e.Time); err != nil {
			return nil, err
		}
		out = append(out, e)
	}
	return out, rows.Err()
}

// ListEpisodes returns rows matching q, ordered by (start_ts, dedup_key).
func ListEpisodes(db *sql.DB, q EpQuery) ([]EpisodeRow, error) {
	sqlStr := `SELECT dedup_key, kind, IFNULL(source,''), start_ts, end_ts, IFNULL(start_offset,''), IFNULL(end_offset,''), IFNULL(body,'{}'), IFNULL(blob_sha256,''), IFNULL(event_id,'') FROM episodes WHERE 1=1`
	var args []any
	if q.Kind != "" {
		sqlStr += ` AND kind = ?`
		args = append(args, q.Kind)
	}
	sqlStr, args = appendWindow(sqlStr, args, q.Window)
	sqlStr += ` ORDER BY start_ts, dedup_key`
	rows, err := db.Query(sqlStr, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []EpisodeRow
	for rows.Next() {
		var e EpisodeRow
		if err := rows.Scan(&e.DedupKey, &e.Kind, &e.Source, &e.Start, &e.End, &e.StartOffset, &e.EndOffset, &e.Body, &e.BlobSHA256, &e.EventID); err != nil {
			return nil, err
		}
		out = append(out, e)
	}
	return out, rows.Err()
}

func appendWindow(q string, args []any, w TimeWindow) (string, []any) {
	if !w.FromUTC.IsZero() {
		q += ` AND start_ts >= ?`
		args = append(args, w.FromUTC.Format("2006-01-02T15:04:05.000000000Z"))
	}
	if !w.ToUTC.IsZero() {
		q += ` AND start_ts < ?`
		args = append(args, w.ToUTC.Format("2006-01-02T15:04:05.000000000Z"))
	}
	return q, args
}

// KindName is one distinct kind and its row count.
type KindName struct {
	Kind  string `json:"kind"`
	Count int    `json:"count"`
	Table string `json:"table"`
}

// ListKinds returns observation and episode kinds, each with a count.
func ListKinds(db *sql.DB) ([]KindName, error) {
	var out []KindName
	for _, table := range []string{"observations", "episodes"} {
		rows, err := db.Query(`SELECT kind, COUNT(*) FROM ` + table + ` GROUP BY kind ORDER BY kind`)
		if err != nil {
			return nil, err
		}
		for rows.Next() {
			var k KindName
			k.Table = table
			if err := rows.Scan(&k.Kind, &k.Count); err != nil {
				rows.Close()
				return nil, err
			}
			out = append(out, k)
		}
		err = rows.Err()
		rows.Close()
		if err != nil {
			return nil, err
		}
	}
	return out, nil
}

// ObservationKindNames is the set of observation.kind values.
func ObservationKindNames(db *sql.DB) ([]string, error) {
	return distinct(db, `SELECT DISTINCT kind FROM observations ORDER BY kind`)
}

// EpisodeKindNames is the set of episode.kind values.
func EpisodeKindNames(db *sql.DB) ([]string, error) {
	return distinct(db, `SELECT DISTINCT kind FROM episodes ORDER BY kind`)
}

// SourceName is one sourceName and how many observation rows it has.
type SourceName struct {
	Source string `json:"source"`
	Count  int    `json:"count"`
}

// ListSources returns observation sources with counts.
func ListSources(db *sql.DB) ([]SourceName, error) {
	rows, err := db.Query(`SELECT source, COUNT(*) FROM observations GROUP BY source ORDER BY source`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []SourceName
	for rows.Next() {
		var s SourceName
		if err := rows.Scan(&s.Source, &s.Count); err != nil {
			return nil, err
		}
		out = append(out, s)
	}
	return out, rows.Err()
}

func distinct(db *sql.DB, q string) ([]string, error) {
	rows, err := db.Query(q)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []string
	for rows.Next() {
		var s string
		if err := rows.Scan(&s); err != nil {
			return nil, err
		}
		out = append(out, s)
	}
	return out, rows.Err()
}

// Stats is archive totals plus the observation date range.
type Stats struct {
	Events       int    `json:"events"`
	Blobs        int    `json:"blobs"`
	Observations int    `json:"observations"`
	Episodes     int    `json:"episodes"`
	From         string `json:"from,omitempty"`
	To           string `json:"to,omitempty"`
}

// ArchiveStats counts rows and the min/max observation start_ts.
func ArchiveStats(db *sql.DB) (Stats, error) {
	c, err := TableCounts(db)
	if err != nil {
		return Stats{}, err
	}
	s := Stats{
		Events:       c.Events,
		Blobs:        c.Blobs,
		Observations: c.Observations,
		Episodes:     c.Episodes,
	}
	var from, to sql.NullString
	if err := db.QueryRow(`SELECT MIN(start_ts), MAX(start_ts) FROM observations`).Scan(&from, &to); err != nil {
		return Stats{}, err
	}
	if from.Valid {
		s.From = from.String
	}
	if to.Valid {
		s.To = to.String
	}
	return s, nil
}

// KindSpan is how many rows a kind has and over what UTC range, ignoring the query window.
type KindSpan struct {
	Kind  string `json:"kind"`
	Table string `json:"table"`
	N     int    `json:"n"`
	From  string `json:"from,omitempty"`
	To    string `json:"to,omitempty"`
}

// LookupKindSpan reports totals for kind in observations, then episodes.
func LookupKindSpan(db *sql.DB, kind string) (KindSpan, error) {
	for _, table := range []string{"observations", "episodes"} {
		var s KindSpan
		s.Kind = kind
		s.Table = table
		var from, to sql.NullString
		q := `SELECT COUNT(*), MIN(start_ts), MAX(start_ts) FROM ` + table + ` WHERE kind = ?`
		if err := db.QueryRow(q, kind).Scan(&s.N, &from, &to); err != nil {
			return KindSpan{}, err
		}
		if s.N == 0 {
			continue
		}
		if from.Valid {
			s.From = from.String
		}
		if to.Valid {
			s.To = to.String
		}
		return s, nil
	}
	return KindSpan{Kind: kind}, nil
}

// FormatSpan is the one-line "this kind exists here" hint.
func (s KindSpan) Format() string {
	if s.N == 0 {
		return fmt.Sprintf("%s  n=0", s.Kind)
	}
	return fmt.Sprintf("%s  n=%d  %s .. %s  (%s)", s.Kind, s.N, s.From, s.To, s.Table)
}

// SumRow is one aggregated observation group.
type SumRow struct {
	Kind   string  `json:"kind,omitempty"`
	Source string  `json:"source,omitempty"`
	N      int     `json:"n"`
	Sum    float64 `json:"sum"`
	Unit   string  `json:"unit,omitempty"`
	Offset string  `json:"offset,omitempty"`
}

// SumObservations adds Value as float64. Without bySource it refuses when
// more than one source is present — summing Watch and phone steps double-counts.
func SumObservations(rows []ObservationRow, bySource bool, offset string) ([]SumRow, error) {
	sources := map[string]bool{}
	for _, r := range rows {
		if r.Source != "" {
			sources[r.Source] = true
		}
	}
	if !bySource && len(sources) > 1 {
		names := make([]string, 0, len(sources))
		for s := range sources {
			names = append(names, s)
		}
		sort.Strings(names)
		return nil, fmt.Errorf("refusing to sum across sources %s; pass -source or -by-source (summing across sources double-counts)", strings.Join(names, ", "))
	}
	type key struct{ kind, source, unit string }
	acc := map[key]*SumRow{}
	order := []key{}
	for _, r := range rows {
		v, err := strconv.ParseFloat(strings.TrimSpace(r.Value), 64)
		if err != nil {
			return nil, fmt.Errorf("sum: non-numeric value %q (%s %s)", r.Value, r.Kind, r.Start)
		}
		src := r.Source
		if !bySource {
			src = ""
		}
		k := key{r.Kind, src, r.Unit}
		if acc[k] == nil {
			acc[k] = &SumRow{Kind: r.Kind, Source: src, Unit: r.Unit, Offset: offset}
			order = append(order, k)
		}
		acc[k].N++
		acc[k].Sum += v
	}
	if !bySource && len(rows) > 0 && len(sources) == 1 {
		for s := range sources {
			for _, k := range order {
				acc[k].Source = s
			}
		}
	}
	out := make([]SumRow, 0, len(order))
	for _, k := range order {
		out = append(out, *acc[k])
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].Source != out[j].Source {
			return out[i].Source < out[j].Source
		}
		return out[i].Kind < out[j].Kind
	})
	return out, nil
}

// Summary is a compact spot-check: counts, range, and per-source sums.
// It never emits a single combined sum across sources.
type Summary struct {
	Kind    string   `json:"kind"`
	N       int      `json:"n"`
	From    string   `json:"from,omitempty"`
	To      string   `json:"to,omitempty"`
	Offset  string   `json:"offset,omitempty"`
	Sources []SumRow `json:"sources,omitempty"`
}

// SummarizeObservations builds a Summary from matching rows.
func SummarizeObservations(rows []ObservationRow, offset string) (Summary, error) {
	s := Summary{Offset: offset, N: len(rows)}
	if len(rows) == 0 {
		return s, nil
	}
	s.Kind = rows[0].Kind
	s.From = rows[0].Start
	s.To = rows[0].Start
	for _, r := range rows {
		if r.Start < s.From {
			s.From = r.Start
		}
		if r.Start > s.To {
			s.To = r.Start
		}
	}
	sums, err := SumObservations(rows, true, offset)
	if err != nil {
		return s, err
	}
	s.Sources = sums
	return s, nil
}
