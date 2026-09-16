// Package projection rebuilds derived/archive.db from log/ + blobs/.
//
// The database is disposable. Change this package and rebuild; never migrate.
package projection

import (
	"database/sql"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"archive/internal/adapters/apple"
	"archive/internal/blobs"
	"archive/internal/event"
	"archive/internal/log"

	_ "github.com/mattn/go-sqlite3"
)

const driver = "sqlite3"

var schema = []string{
	`PRAGMA journal_mode = OFF`,
	`PRAGMA synchronous = OFF`,
	`PRAGMA page_size = 4096`,
	`PRAGMA encoding = 'UTF-8'`,
	`PRAGMA user_version = 0`,
	`PRAGMA application_id = 1095913288`, // 'ARCH'
	`CREATE TABLE events (
  id TEXT PRIMARY KEY,
  ts TEXT NOT NULL,
  dev TEXT NOT NULL,
  type TEXT NOT NULL,
  body TEXT NOT NULL,
  superseded_by TEXT,
  retracted INTEGER NOT NULL DEFAULT 0
)`,
	`CREATE INDEX events_ts ON events(ts)`,
	`CREATE INDEX events_type ON events(type)`,
	`CREATE INDEX events_dev ON events(dev)`,
	`CREATE TABLE blobs (
  sha256 TEXT PRIMARY KEY,
  size INTEGER NOT NULL,
  name TEXT NOT NULL
)`,
	`CREATE TABLE observations (
  dedup_key TEXT NOT NULL,
  n INTEGER NOT NULL,
  kind TEXT NOT NULL,
  source TEXT NOT NULL,
  source_version TEXT,
  device TEXT,
  start_ts TEXT NOT NULL,
  end_ts TEXT NOT NULL,
  start_offset TEXT,
  end_offset TEXT,
  unit TEXT,
  value TEXT,
  metadata TEXT NOT NULL DEFAULT '{}',
  blob_sha256 TEXT,
  event_id TEXT,
  PRIMARY KEY (dedup_key, n)
)`,
	`CREATE INDEX observations_kind_start ON observations(kind, start_ts)`,
	`CREATE INDEX observations_source ON observations(source)`,
	`CREATE TABLE episodes (
  dedup_key TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  source TEXT,
  start_ts TEXT NOT NULL,
  end_ts TEXT NOT NULL,
  start_offset TEXT,
  end_offset TEXT,
  body TEXT NOT NULL,
  blob_sha256 TEXT,
  event_id TEXT,
  route_polyline BLOB,
  route_segments INTEGER,
  route_points INTEGER,
  route_bbox TEXT,
  ascent_m REAL,
  descent_m REAL,
  moving_seconds INTEGER,
  elapsed_seconds INTEGER,
  pause_count INTEGER
)`,
	`CREATE INDEX episodes_kind_start ON episodes(kind, start_ts)`,
	`CREATE TABLE workout_event (
  episode_id TEXT NOT NULL,
  seq INTEGER NOT NULL,
  kind TEXT NOT NULL,
  time_utc TEXT NOT NULL,
  PRIMARY KEY (episode_id, seq)
)`,
	`CREATE INDEX workout_event_episode ON workout_event(episode_id, time_utc)`,
	`CREATE TABLE source_precedence (
  source_name TEXT PRIMARY KEY,
  rank INTEGER NOT NULL
)`,
	`CREATE TABLE attachments (
  blob_sha256 TEXT NOT NULL,
  path TEXT NOT NULL,
  size INTEGER NOT NULL,
  PRIMARY KEY (blob_sha256, path)
)`,
	`CREATE TABLE apple_exports (
  sha256 TEXT PRIMARY KEY,
  locale TEXT,
  export_date TEXT,
  me TEXT NOT NULL
)`,
	`CREATE VIEW preferred_observations AS
SELECT o.* FROM observations o
JOIN source_precedence sp ON o.source = sp.source_name
WHERE NOT EXISTS (
  SELECT 1 FROM observations o2
  JOIN source_precedence sp2 ON o2.source = sp2.source_name
  WHERE o2.kind = o.kind
    AND o2.start_ts = o.start_ts
    AND o2.end_ts = o.end_ts
    AND (sp2.rank < sp.rank OR (sp2.rank = sp.rank AND o2.source < o.source))
)`,
}

// DBPath is the derived database location under root.
func DBPath(root string) string {
	return filepath.Join(root, "derived", "archive.db")
}

// Rebuild deletes derived/archive.db and reconstructs it from the log.
func Rebuild(root string) error {
	_, err := RebuildReport(root)
	return err
}

// RebuildReport reconstructs the projection from every complete log line and
// returns any torn-tail diagnostics. It never repairs the log.
func RebuildReport(root string) (log.Report, error) {
	rep, err := log.ReadReport(root)
	if err != nil {
		return log.Report{}, err
	}
	return rep, rebuild(root, rep.Events)
}

func rebuild(root string, evs []event.Event) error {
	path := DBPath(root)
	for _, p := range []string{path, path + "-wal", path + "-shm", path + "-journal"} {
		if err := os.Remove(p); err != nil && !os.IsNotExist(err) {
			return err
		}
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return err
	}

	db, err := sql.Open(driver, path)
	if err != nil {
		return err
	}
	db.SetMaxOpenConns(1)
	defer db.Close()

	for _, stmt := range schema {
		if _, err := db.Exec(stmt); err != nil {
			return fmt.Errorf("schema %q: %w", stmt, err)
		}
	}

	tx, err := db.Begin()
	if err != nil {
		return err
	}
	if err := insertEvents(tx, evs); err != nil {
		tx.Rollback()
		return err
	}
	if err := applyCorrections(tx, evs); err != nil {
		tx.Rollback()
		return err
	}
	if err := projectBlobs(tx, root, evs); err != nil {
		tx.Rollback()
		return err
	}
	if err := tx.Commit(); err != nil {
		return err
	}
	if _, err := db.Exec(`VACUUM`); err != nil {
		return err
	}
	return chmodDB(path)
}

func chmodDB(path string) error {
	for _, p := range []string{path, path + "-wal", path + "-shm"} {
		if err := os.Chmod(p, 0o600); err != nil && !os.IsNotExist(err) {
			return err
		}
	}
	return nil
}

func insertEvents(tx *sql.Tx, evs []event.Event) error {
	insEv, err := tx.Prepare(`INSERT INTO events (id, ts, dev, type, body) VALUES (?, ?, ?, ?, ?)`)
	if err != nil {
		return err
	}
	defer insEv.Close()
	insBlob, err := tx.Prepare(`INSERT OR IGNORE INTO blobs (sha256, size, name) VALUES (?, ?, ?)`)
	if err != nil {
		return err
	}
	defer insBlob.Close()

	for _, ev := range evs {
		body := ev.Body
		if len(body) == 0 {
			body = json.RawMessage("{}")
		}
		if _, err := insEv.Exec(ev.ID, ev.TS, ev.Dev, ev.Type, string(body)); err != nil {
			return err
		}
		if ev.Type != event.TypeBlobImport {
			continue
		}
		b, err := event.ParseBlobImport(ev.Body)
		if err != nil {
			return fmt.Errorf("blob_import %s: %w", ev.ID, err)
		}
		if _, err := insBlob.Exec(b.SHA256, b.Size, b.Name); err != nil {
			return err
		}
	}
	return nil
}

// voidedIDs is the set of event ids retracted or superseded after
// last-writer-wins application of corrections (same flags applyCorrections writes).
func voidedIDs(tx *sql.Tx) (map[string]bool, error) {
	rows, err := tx.Query(`SELECT id FROM events WHERE retracted = 1 OR superseded_by IS NOT NULL`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := map[string]bool{}
	for rows.Next() {
		var id string
		if err := rows.Scan(&id); err != nil {
			return nil, err
		}
		out[id] = true
	}
	return out, rows.Err()
}

func projectBlobs(tx *sql.Tx, root string, evs []event.Event) error {
	voided, err := voidedIDs(tx)
	if err != nil {
		return err
	}
	sources := map[string]int{}
	for _, ev := range evs {
		if ev.Type != event.TypeBlobImport {
			continue
		}
		if voided[ev.ID] {
			continue
		}
		b, err := event.ParseBlobImport(ev.Body)
		if err != nil {
			return err
		}
		path, err := blobs.Path(root, b.SHA256)
		if err != nil {
			return err
		}
		if _, err := os.Stat(path); err != nil {
			continue
		}
		if b.Kind != apple.KindApple && !apple.IsExport(path) {
			continue
		}
		if err := projectApple(tx, path, b.SHA256, ev.ID, sources); err != nil {
			return fmt.Errorf("apple %s: %w", b.SHA256[:12], err)
		}
	}
	seen := map[string]bool{}
	for _, n := range apple.ConfiguredSources() {
		if _, err := tx.Exec(`INSERT INTO source_precedence (source_name, rank) VALUES (?, ?)`, n, apple.SourceRank(n)); err != nil {
			return err
		}
		seen[n] = true
	}
	extras := make([]string, 0)
	for n := range sources {
		if !seen[n] {
			extras = append(extras, n)
		}
	}
	sort.Strings(extras)
	for _, n := range extras {
		if _, err := tx.Exec(`INSERT INTO source_precedence (source_name, rank) VALUES (?, ?)`, n, apple.SourceRank(n)); err != nil {
			return err
		}
	}
	return nil
}

func projectApple(tx *sql.Tx, path, sha, eventID string, sources map[string]int) error {
	insObs, err := tx.Prepare(`INSERT INTO observations
		(dedup_key, n, kind, source, source_version, device, start_ts, end_ts, start_offset, end_offset, unit, value, metadata, blob_sha256, event_id)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`)
	if err != nil {
		return err
	}
	defer insObs.Close()
	// Per-key counts already in the DB from earlier blob_imports this rebuild.
	have := map[string]int{}
	rows, err := tx.Query(`SELECT dedup_key, COUNT(*) FROM observations GROUP BY dedup_key`)
	if err != nil {
		return err
	}
	for rows.Next() {
		var k string
		var c int
		if err := rows.Scan(&k, &c); err != nil {
			rows.Close()
			return err
		}
		have[k] = c
	}
	if err := rows.Close(); err != nil {
		return err
	}
	insEp, err := tx.Prepare(`INSERT OR IGNORE INTO episodes
		(dedup_key, kind, source, start_ts, end_ts, start_offset, end_offset, body, blob_sha256, event_id,
		 route_polyline, route_segments, route_points, route_bbox, ascent_m, descent_m, moving_seconds, elapsed_seconds, pause_count)
		VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)`)
	if err != nil {
		return err
	}
	defer insEp.Close()
	insWE, err := tx.Prepare(`INSERT OR IGNORE INTO workout_event (episode_id, seq, kind, time_utc) VALUES (?, ?, ?, ?)`)
	if err != nil {
		return err
	}
	defer insWE.Close()

	rc, err := apple.OpenXML(path)
	if err != nil {
		return err
	}
	defer rc.Close()

	seen := map[string]int{}
	hdr, err := apple.Stream(rc, apple.Handler{
		Observation: func(o apple.Observation) error {
			if o.Source != "" {
				sources[o.Source] = apple.SourceRank(o.Source)
			}
			seen[o.DedupKey]++
			if seen[o.DedupKey] <= have[o.DedupKey] {
				return nil
			}
			meta, err := marshalMeta(o)
			if err != nil {
				return err
			}
			n := have[o.DedupKey] + 1
			if _, err := insObs.Exec(o.DedupKey, n, o.Kind, o.Source, o.SourceVersion, o.Device,
				o.Start, o.End, o.StartOffset, o.EndOffset, o.Unit, o.Value, meta, sha, eventID); err != nil {
				return err
			}
			have[o.DedupKey] = n
			return nil
		},
		Episode: func(e apple.Episode) error {
			if e.Source != "" {
				sources[e.Source] = apple.SourceRank(e.Source)
			}
			body, err := json.Marshal(e.Body)
			if err != nil {
				return err
			}
			rt := enrichEpisode(path, e)
			timed := strings.HasPrefix(e.Kind, "HKWorkoutActivityType")
			hasPoly := len(rt.Polyline) > 0
			_, err = insEp.Exec(e.DedupKey, e.Kind, e.Source, e.Start, e.End, e.StartOffset, e.EndOffset, string(body), sha, eventID,
				nullBlob(rt.Polyline), nullInt(rt.Segments, hasPoly), nullInt(rt.Points, hasPoly),
				nullStr(rt.BBox), nullFloat(rt.AscentM), nullFloat(rt.DescentM),
				nullInt(rt.MovingS, timed), nullInt(rt.ElapsedS, timed), nullInt(rt.PauseN, timed))
			if err != nil {
				return err
			}
			return insertWorkoutEvents(insWE, e)
		},
	})
	if err != nil {
		return err
	}
	if hdr.Me == nil {
		hdr.Me = map[string]string{}
	}
	me, err := json.Marshal(hdr.Me)
	if err != nil {
		return err
	}
	if _, err := tx.Exec(`INSERT OR IGNORE INTO apple_exports (sha256, locale, export_date, me) VALUES (?, ?, ?, ?)`,
		sha, hdr.Locale, hdr.ExportDate, string(me)); err != nil {
		return err
	}

	atts, err := apple.ListAttachments(path)
	if err != nil {
		return err
	}
	for _, a := range atts {
		if _, err := tx.Exec(`INSERT OR IGNORE INTO attachments (blob_sha256, path, size) VALUES (?, ?, ?)`,
			sha, a.Path, a.Size); err != nil {
			return err
		}
	}
	return nil
}

func marshalMeta(o apple.Observation) (string, error) {
	m := map[string]any{}
	if len(o.Metadata) > 0 {
		m["entries"] = o.Metadata
	}
	if len(o.HRV) > 0 {
		m["hrv"] = o.HRV
	}
	if len(m) == 0 {
		return "{}", nil
	}
	b, err := json.Marshal(m)
	return string(b), err
}

func applyCorrections(tx *sql.Tx, evs []event.Event) error {
	for _, ev := range evs {
		if ev.Type != event.TypeSupersede && ev.Type != event.TypeRetract {
			continue
		}
		target, ok := ev.Target()
		if !ok {
			return fmt.Errorf("%s %s: missing target", ev.Type, ev.ID)
		}
		switch ev.Type {
		case event.TypeSupersede:
			if _, err := tx.Exec(`UPDATE events SET superseded_by = ?, retracted = 0 WHERE id = ?`, ev.ID, target); err != nil {
				return err
			}
		case event.TypeRetract:
			if _, err := tx.Exec(`UPDATE events SET retracted = 1, superseded_by = NULL WHERE id = ?`, target); err != nil {
				return err
			}
		}
	}
	return nil
}

// Open opens the derived database for queries.
func Open(root string) (*sql.DB, error) {
	path := DBPath(root)
	if _, err := os.Stat(path); err != nil {
		return nil, fmt.Errorf("query: database missing; run archive rebuild")
	}
	db, err := sql.Open(driver, path)
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	return db, nil
}

// Filter selects events from the projection.
type Filter struct {
	Type    string
	Dev     string
	ID      string
	Current bool
}

// Query returns events matching f, ordered by (ts, id).
func Query(db *sql.DB, f Filter) ([]event.Event, error) {
	q := `SELECT id, ts, dev, type, body FROM events WHERE 1=1`
	var args []any
	if f.Type != "" {
		q += ` AND type = ?`
		args = append(args, f.Type)
	}
	if f.Dev != "" {
		q += ` AND dev = ?`
		args = append(args, f.Dev)
	}
	if f.ID != "" {
		q += ` AND id = ?`
		args = append(args, f.ID)
	}
	if f.Current {
		q += ` AND retracted = 0 AND superseded_by IS NULL`
	}
	q += ` ORDER BY ts, id`

	rows, err := db.Query(q, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()

	var out []event.Event
	for rows.Next() {
		var ev event.Event
		var body string
		if err := rows.Scan(&ev.ID, &ev.TS, &ev.Dev, &ev.Type, &body); err != nil {
			return nil, err
		}
		ev.Body = json.RawMessage(body)
		out = append(out, ev)
	}
	return out, rows.Err()
}

// ObservationRow is one projected Record.
type ObservationRow struct {
	DedupKey      string `json:"dedup_key"`
	N             int    `json:"n"`
	Kind          string `json:"kind"`
	Source        string `json:"source"`
	SourceVersion string `json:"source_version,omitempty"`
	Device        string `json:"device,omitempty"`
	Start         string `json:"start_ts"`
	End           string `json:"end_ts"`
	StartOffset   string `json:"start_offset,omitempty"`
	EndOffset     string `json:"end_offset,omitempty"`
	Unit          string `json:"unit,omitempty"`
	Value         string `json:"value,omitempty"`
	Metadata      string `json:"metadata"`
	BlobSHA256    string `json:"blob_sha256,omitempty"`
	EventID       string `json:"event_id,omitempty"`
}

// QueryObservations returns observations ordered by (start_ts, dedup_key, n).
func QueryObservations(db *sql.DB, kind, source string) ([]ObservationRow, error) {
	return ListObservations(db, ObsQuery{Kind: kind, Source: source})
}

// EpisodeRow is one projected Workout, Correlation, or ActivitySummary.
type EpisodeRow struct {
	DedupKey       string   `json:"dedup_key"`
	Kind           string   `json:"kind"`
	Source         string   `json:"source,omitempty"`
	Start          string   `json:"start_ts"`
	End            string   `json:"end_ts"`
	StartOffset    string   `json:"start_offset,omitempty"`
	EndOffset      string   `json:"end_offset,omitempty"`
	Body           string   `json:"body"`
	BlobSHA256     string   `json:"blob_sha256,omitempty"`
	EventID        string   `json:"event_id,omitempty"`
	RoutePolyline  []byte   `json:"-"`
	RouteSegments  int      `json:"route_segments,omitempty"`
	RoutePoints    int      `json:"route_points,omitempty"`
	RouteBBox      string   `json:"route_bbox,omitempty"`
	AscentM        *float64 `json:"ascent_m,omitempty"`
	DescentM       *float64 `json:"descent_m,omitempty"`
	MovingSeconds  *int     `json:"moving_seconds,omitempty"`
	ElapsedSeconds *int     `json:"elapsed_seconds,omitempty"`
	PauseCount     *int     `json:"pause_count,omitempty"`
}

// WorkoutEventRow is one projected WorkoutEvent.
type WorkoutEventRow struct {
	Kind string
	Time string
}

// QueryEpisodes returns episodes ordered by (start_ts, dedup_key).
func QueryEpisodes(db *sql.DB, kind string) ([]EpisodeRow, error) {
	return ListEpisodes(db, EpQuery{Kind: kind})
}

// Counts is row totals after a rebuild.
type Counts struct {
	Events       int
	Blobs        int
	Observations int
	Episodes     int
}

// TableCounts returns events, blobs, observations, and episodes row totals.
func TableCounts(db *sql.DB) (Counts, error) {
	var c Counts
	queries := []struct {
		q    string
		dest *int
	}{
		{`SELECT COUNT(*) FROM events`, &c.Events},
		{`SELECT COUNT(*) FROM blobs`, &c.Blobs},
		{`SELECT COUNT(*) FROM observations`, &c.Observations},
		{`SELECT COUNT(*) FROM episodes`, &c.Episodes},
	}
	for _, q := range queries {
		if err := db.QueryRow(q.q).Scan(q.dest); err != nil {
			return Counts{}, err
		}
	}
	return c, nil
}

// ObservationCounts returns row counts per Apple type identifier.
func ObservationCounts(db *sql.DB) (map[string]int, error) {
	rows, err := db.Query(`SELECT kind, COUNT(*) FROM observations GROUP BY kind ORDER BY kind`)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := map[string]int{}
	for rows.Next() {
		var k string
		var n int
		if err := rows.Scan(&k, &n); err != nil {
			return nil, err
		}
		out[k] = n
	}
	return out, rows.Err()
}
