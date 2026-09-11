package projection

import (
	"database/sql"
	"sort"
	"strings"
	"time"

	"archive/internal/event"
)

// KindInfo is one observation or episode kind as the UI catalog needs it.
// Computed from existing tables — no schema change.
type KindInfo struct {
	Kind      string   `json:"kind"`
	Table     string   `json:"table"`
	N         int      `json:"n"`
	From      string   `json:"from,omitempty"`
	To        string   `json:"to,omitempty"`
	Unit      string   `json:"unit,omitempty"`
	Sources   []string `json:"sources,omitempty"`
	Preferred string   `json:"preferred,omitempty"`
	// Overlaps is true when two sources recorded this kind on the same
	// local day. Those kinds need a preferred source. Complementary
	// sources (never the same day) do not.
	Overlaps bool `json:"overlaps,omitempty"`
}

// KindCatalog lists every projected kind with counts, range, and sources.
func KindCatalog(db *sql.DB) ([]KindInfo, error) {
	obs, err := kindRows(db, "observations")
	if err != nil {
		return nil, err
	}
	eps, err := kindRows(db, "episodes")
	if err != nil {
		return nil, err
	}
	out := append(obs, eps...)
	sort.Slice(out, func(i, j int) bool {
		if out[i].Table != out[j].Table {
			return out[i].Table == "observations"
		}
		return ShortKind(out[i].Kind) < ShortKind(out[j].Kind)
	})
	return out, nil
}

func kindRows(db *sql.DB, table string) ([]KindInfo, error) {
	q := `SELECT kind, COUNT(*), MIN(start_ts), MAX(start_ts) FROM ` + table + ` GROUP BY kind`
	rows, err := db.Query(q)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var out []KindInfo
	for rows.Next() {
		var k KindInfo
		k.Table = table
		var from, to sql.NullString
		if err := rows.Scan(&k.Kind, &k.N, &from, &to); err != nil {
			return nil, err
		}
		if from.Valid {
			k.From = from.String
		}
		if to.Valid {
			k.To = to.String
		}
		out = append(out, k)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	if table == "observations" {
		if err := attachObservationSources(db, out); err != nil {
			return nil, err
		}
		if err := attachOverlaps(db, out); err != nil {
			return nil, err
		}
	} else {
		if err := attachEpisodeSources(db, out); err != nil {
			return nil, err
		}
	}
	return out, nil
}

func attachObservationSources(db *sql.DB, kinds []KindInfo) error {
	rows, err := db.Query(`
SELECT o.kind, o.source, COUNT(*), IFNULL(sp.rank, 100)
FROM observations o
LEFT JOIN source_precedence sp ON o.source = sp.source_name
GROUP BY o.kind, o.source
ORDER BY o.kind, IFNULL(sp.rank, 100), o.source`)
	if err != nil {
		return err
	}
	defer rows.Close()
	byKind := map[string][]string{}
	pref := map[string]string{}
	units := map[string]string{}
	for rows.Next() {
		var kind, source string
		var n, rank int
		if err := rows.Scan(&kind, &source, &n, &rank); err != nil {
			return err
		}
		if source != "" {
			byKind[kind] = append(byKind[kind], source)
			if pref[kind] == "" {
				pref[kind] = source
			}
		}
	}
	if err := rows.Err(); err != nil {
		return err
	}
	urows, err := db.Query(`SELECT kind, MIN(unit) FROM observations WHERE unit IS NOT NULL AND unit != '' GROUP BY kind`)
	if err != nil {
		return err
	}
	defer urows.Close()
	for urows.Next() {
		var kind, unit string
		if err := urows.Scan(&kind, &unit); err != nil {
			return err
		}
		units[kind] = unit
	}
	if err := urows.Err(); err != nil {
		return err
	}
	for i := range kinds {
		kinds[i].Sources = byKind[kinds[i].Kind]
		kinds[i].Preferred = pref[kinds[i].Kind]
		kinds[i].Unit = units[kinds[i].Kind]
	}
	return nil
}

func attachEpisodeSources(db *sql.DB, kinds []KindInfo) error {
	rows, err := db.Query(`
SELECT kind, IFNULL(source,''), COUNT(*)
FROM episodes
GROUP BY kind, source
ORDER BY kind, source`)
	if err != nil {
		return err
	}
	defer rows.Close()
	byKind := map[string][]string{}
	for rows.Next() {
		var kind, source string
		var n int
		if err := rows.Scan(&kind, &source, &n); err != nil {
			return err
		}
		if source != "" {
			byKind[kind] = append(byKind[kind], source)
		}
	}
	if err := rows.Err(); err != nil {
		return err
	}
	for i := range kinds {
		kinds[i].Sources = byKind[kinds[i].Kind]
		if len(kinds[i].Sources) > 0 {
			kinds[i].Preferred = kinds[i].Sources[0]
		}
	}
	return nil
}

func attachOverlaps(db *sql.DB, kinds []KindInfo) error {
	loc, err := DisplayLocation()
	if err != nil {
		loc = time.UTC
	}
	var multi []string
	for _, k := range kinds {
		if len(k.Sources) > 1 {
			multi = append(multi, k.Kind)
		}
	}
	if len(multi) == 0 {
		return nil
	}
	args := make([]any, len(multi))
	ph := make([]string, len(multi))
	for i, k := range multi {
		args[i] = k
		ph[i] = "?"
	}
	// MIN/MAX per UTC day, then convert to local. A sample just after
	// midnight local lives on the previous UTC date; using only date()
	// would miss the collision.
	q := `SELECT kind, source, MIN(start_ts), MAX(start_ts) FROM observations
WHERE kind IN (` + strings.Join(ph, ",") + `)
GROUP BY kind, source, substr(start_ts, 1, 10)`
	rows, err := db.Query(q, args...)
	if err != nil {
		return err
	}
	defer rows.Close()
	seen := map[string]map[string]map[string]struct{}{}
	hit := map[string]bool{}
	for rows.Next() {
		var kind, source, minTS, maxTS string
		if err := rows.Scan(&kind, &source, &minTS, &maxTS); err != nil {
			return err
		}
		if source == "" {
			continue
		}
		a, err1 := localDay(minTS, loc)
		b, err2 := localDay(maxTS, loc)
		if err1 != nil || err2 != nil {
			continue
		}
		if seen[kind] == nil {
			seen[kind] = map[string]map[string]struct{}{}
		}
		for _, day := range daysInclusive(a, b) {
			if seen[kind][day] == nil {
				seen[kind][day] = map[string]struct{}{}
			}
			seen[kind][day][source] = struct{}{}
			if len(seen[kind][day]) > 1 {
				hit[kind] = true
			}
		}
	}
	if err := rows.Err(); err != nil {
		return err
	}
	for i := range kinds {
		kinds[i].Overlaps = hit[kinds[i].Kind]
	}
	return nil
}

func daysInclusive(a, b string) []string {
	if a == "" {
		return nil
	}
	if b == "" || b == a {
		return []string{a}
	}
	if a > b {
		a, b = b, a
	}
	start, err1 := time.Parse("2006-01-02", a)
	end, err2 := time.Parse("2006-01-02", b)
	if err1 != nil || err2 != nil {
		return []string{a, b}
	}
	var out []string
	for d := start; !d.After(end); d = d.AddDate(0, 0, 1) {
		out = append(out, d.Format("2006-01-02"))
	}
	return out
}

// BlobImportRow is one imported source file as shown on the blobs page.
type BlobImportRow struct {
	SHA256 string `json:"sha256"`
	Size   int64  `json:"size"`
	Name   string `json:"name"`
	Kind   string `json:"kind,omitempty"`
	TS     string `json:"ts"`
}

// ListBlobImports returns blob_import events joined with the blobs table.
func ListBlobImports(db *sql.DB) ([]BlobImportRow, error) {
	evs, err := Query(db, Filter{Type: event.TypeBlobImport})
	if err != nil {
		return nil, err
	}
	out := make([]BlobImportRow, 0, len(evs))
	for _, ev := range evs {
		b, err := event.ParseBlobImport(ev.Body)
		if err != nil {
			return nil, err
		}
		out = append(out, BlobImportRow{
			SHA256: b.SHA256,
			Size:   b.Size,
			Name:   b.Name,
			Kind:   b.Kind,
			TS:     ev.TS,
		})
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].TS != out[j].TS {
			return out[i].TS > out[j].TS
		}
		return out[i].SHA256 < out[j].SHA256
	})
	return out, nil
}

// MultiSourceKinds returns observation kinds that have more than one source.
func MultiSourceKinds(catalog []KindInfo) []KindInfo {
	var out []KindInfo
	for _, k := range catalog {
		if k.Table == "observations" && len(k.Sources) > 1 {
			out = append(out, k)
		}
	}
	return out
}

// FindKind returns the catalog entry for kind, or a zero value.
func FindKind(catalog []KindInfo, kind string) KindInfo {
	for _, k := range catalog {
		if k.Kind == kind {
			return k
		}
	}
	return KindInfo{Kind: kind}
}

// HasSource reports whether name is one of the kind's sources.
func (k KindInfo) HasSource(name string) bool {
	for _, s := range k.Sources {
		if s == name {
			return true
		}
	}
	return false
}

// SourceList is a comma-separated source list for templates.
func (k KindInfo) SourceList() string {
	return strings.Join(k.Sources, ", ")
}
