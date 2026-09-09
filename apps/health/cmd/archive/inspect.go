package main

import (
	"database/sql"
	"encoding/json"
	"flag"
	"fmt"
	"io"
	"strings"

	"archive/internal/projection"
)

func cmdObservations(stdout, stderr io.Writer, root string, args []string) error {
	fs := flag.NewFlagSet("observations", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	kind := fs.String("kind", "", "Apple type or unique suffix (StepCount, SleepAnalysis)")
	source := fs.String("source", "", "sourceName")
	on := fs.String("on", "", "local calendar day YYYY-MM-DD")
	from := fs.String("from", "", "local start day YYYY-MM-DD inclusive")
	to := fs.String("to", "", "local end day YYYY-MM-DD inclusive")
	sum := fs.Bool("sum", false, "sum value (refuses multiple sources unless -by-source)")
	bySource := fs.Bool("by-source", false, "one SUM/COUNT row per source (implies aggregation)")
	summary := fs.Bool("summary", false, "counts, per-source sums, first/last timestamp")
	asJSON := fs.Bool("json", false, "JSON output")
	if err := fs.Parse(args); err != nil {
		return fmt.Errorf("observations: %w", err)
	}
	db, win, err := openWindow(root, *on, *from, *to, stderr)
	if err != nil {
		return err
	}
	defer db.Close()
	resolved, err := projection.ResolveKindInDB(db, *kind, "observations")
	if err != nil {
		return err
	}
	if *kind != "" && !resolved.Observations && resolved.Episodes {
		fmt.Fprintf(stderr, "note: %s is an episode, not an observation\n", resolved.Kind)
		rows, err := projection.ListEpisodes(db, projection.EpQuery{Kind: resolved.Kind, Window: win})
		if err != nil {
			return err
		}
		return writeEpisodeResult(stdout, stderr, db, resolved.Kind, rows, win, *asJSON)
	}
	rows, err := projection.ListObservations(db, projection.ObsQuery{Kind: resolved.Kind, Source: *source, Window: win})
	if err != nil {
		return err
	}
	if len(rows) == 0 {
		return writeEmpty(stdout, db, resolved.Kind, *asJSON)
	}
	if *summary {
		summ, err := projection.SummarizeObservations(rows, win.Offset)
		if err != nil {
			return err
		}
		return writeSummary(stdout, summ, *asJSON)
	}
	if *sum || *bySource {
		sums, err := projection.SumObservations(rows, *bySource, win.Offset)
		if err != nil {
			return err
		}
		return writeSums(stdout, sums, *asJSON)
	}
	if *asJSON {
		return writeJSONLines(stdout, rows)
	}
	for _, r := range rows {
		fmt.Fprintf(stdout, "%s  %s  %s %s  %s\n", r.Start, r.Source, r.Value, r.Unit, shortKind(r.Kind))
	}
	return nil
}

func cmdEpisodes(stdout, stderr io.Writer, root string, args []string) error {
	fs := flag.NewFlagSet("episodes", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	kind := fs.String("kind", "", "Apple type or unique suffix (Running, SleepAnalysis)")
	on := fs.String("on", "", "local calendar day YYYY-MM-DD")
	from := fs.String("from", "", "local start day YYYY-MM-DD inclusive")
	to := fs.String("to", "", "local end day YYYY-MM-DD inclusive")
	asJSON := fs.Bool("json", false, "JSON output")
	if err := fs.Parse(args); err != nil {
		return fmt.Errorf("episodes: %w", err)
	}
	db, win, err := openWindow(root, *on, *from, *to, stderr)
	if err != nil {
		return err
	}
	defer db.Close()
	resolved, err := projection.ResolveKindInDB(db, *kind, "episodes")
	if err != nil {
		return err
	}
	if *kind != "" && !resolved.Episodes && resolved.Observations {
		fmt.Fprintf(stderr, "note: %s is an observation (category Record), not an episode\n", resolved.Kind)
		rows, err := projection.ListObservations(db, projection.ObsQuery{Kind: resolved.Kind, Window: win})
		if err != nil {
			return err
		}
		if len(rows) == 0 {
			return writeEmpty(stdout, db, resolved.Kind, *asJSON)
		}
		if *asJSON {
			return writeJSONLines(stdout, rows)
		}
		for _, r := range rows {
			fmt.Fprintf(stdout, "%s  %s  %s  %s\n", r.Start, r.Source, r.Value, shortKind(r.Kind))
		}
		return nil
	}
	rows, err := projection.ListEpisodes(db, projection.EpQuery{Kind: resolved.Kind, Window: win})
	if err != nil {
		return err
	}
	return writeEpisodeResult(stdout, stderr, db, resolved.Kind, rows, win, *asJSON)
}

func writeEpisodeResult(stdout, stderr io.Writer, db *sql.DB, kind string, rows []projection.EpisodeRow, win projection.TimeWindow, asJSON bool) error {
	_ = stderr
	_ = win
	if len(rows) == 0 {
		return writeEmpty(stdout, db, kind, asJSON)
	}
	if asJSON {
		return writeJSONLines(stdout, rows)
	}
	for _, r := range rows {
		fmt.Fprintf(stdout, "%s  %s  %s\n", r.Start, r.Source, shortKind(r.Kind))
	}
	return nil
}

func cmdKinds(stdout io.Writer, root string, args []string) error {
	fs := flag.NewFlagSet("kinds", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	count := fs.Bool("count", false, "include row counts")
	asJSON := fs.Bool("json", false, "JSON output")
	if err := fs.Parse(args); err != nil {
		return fmt.Errorf("kinds: %w", err)
	}
	db, err := projection.Open(root)
	if err != nil {
		return err
	}
	defer db.Close()
	rows, err := projection.ListKinds(db)
	if err != nil {
		return err
	}
	if *asJSON {
		return writeJSONLines(stdout, rows)
	}
	for _, r := range rows {
		if *count {
			fmt.Fprintf(stdout, "%6d  %s\n", r.Count, r.Kind)
		} else {
			fmt.Fprintln(stdout, r.Kind)
		}
	}
	return nil
}

func cmdSources(stdout io.Writer, root string, args []string) error {
	fs := flag.NewFlagSet("sources", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	count := fs.Bool("count", false, "include row counts")
	asJSON := fs.Bool("json", false, "JSON output")
	if err := fs.Parse(args); err != nil {
		return fmt.Errorf("sources: %w", err)
	}
	db, err := projection.Open(root)
	if err != nil {
		return err
	}
	defer db.Close()
	rows, err := projection.ListSources(db)
	if err != nil {
		return err
	}
	if *asJSON {
		return writeJSONLines(stdout, rows)
	}
	for _, r := range rows {
		if *count {
			fmt.Fprintf(stdout, "%6d  %s\n", r.Count, r.Source)
		} else {
			fmt.Fprintln(stdout, r.Source)
		}
	}
	return nil
}

func cmdStats(stdout io.Writer, root string, args []string) error {
	fs := flag.NewFlagSet("stats", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	asJSON := fs.Bool("json", false, "JSON output")
	if err := fs.Parse(args); err != nil {
		return fmt.Errorf("stats: %w", err)
	}
	db, err := projection.Open(root)
	if err != nil {
		return err
	}
	defer db.Close()
	s, err := projection.ArchiveStats(db)
	if err != nil {
		return err
	}
	if *asJSON {
		enc := json.NewEncoder(stdout)
		enc.SetEscapeHTML(false)
		return enc.Encode(s)
	}
	fmt.Fprintf(stdout, "events         %d\n", s.Events)
	fmt.Fprintf(stdout, "blobs          %d\n", s.Blobs)
	fmt.Fprintf(stdout, "observations   %d\n", s.Observations)
	fmt.Fprintf(stdout, "episodes       %d\n", s.Episodes)
	if s.From != "" {
		fmt.Fprintf(stdout, "from           %s\n", s.From)
		fmt.Fprintf(stdout, "to             %s\n", s.To)
	}
	return nil
}

func openWindow(root, on, from, to string, stderr io.Writer) (*sql.DB, projection.TimeWindow, error) {
	loc, err := projection.DisplayLocation()
	if err != nil {
		return nil, projection.TimeWindow{}, err
	}
	win, err := projection.LocalWindow(on, from, to, loc)
	if err != nil {
		return nil, projection.TimeWindow{}, err
	}
	if note := win.Note(); note != "" {
		fmt.Fprintln(stderr, note)
	}
	db, err := projection.Open(root)
	if err != nil {
		return nil, projection.TimeWindow{}, err
	}
	return db, win, nil
}

func writeEmpty(w io.Writer, db *sql.DB, kind string, asJSON bool) error {
	span, err := projection.LookupKindSpan(db, kind)
	if err != nil {
		return err
	}
	if asJSON {
		enc := json.NewEncoder(w)
		enc.SetEscapeHTML(false)
		return enc.Encode(struct {
			Matches int                 `json:"matches"`
			Span    projection.KindSpan `json:"span"`
		}{0, span})
	}
	fmt.Fprintln(w, "no matching rows")
	if kind != "" {
		fmt.Fprintln(w, span.Format())
	}
	return nil
}

func writeSummary(w io.Writer, s projection.Summary, asJSON bool) error {
	if asJSON {
		enc := json.NewEncoder(w)
		enc.SetEscapeHTML(false)
		return enc.Encode(s)
	}
	fmt.Fprintf(w, "kind %s\nn %d\nfrom %s\nto %s\n", s.Kind, s.N, s.From, s.To)
	if s.Offset != "" {
		fmt.Fprintf(w, "offset %s\n", s.Offset)
	}
	for _, r := range s.Sources {
		fmt.Fprintf(w, "%s  n=%d  sum=%g  %s\n", r.Source, r.N, r.Sum, r.Unit)
	}
	return nil
}

func writeSums(w io.Writer, rows []projection.SumRow, asJSON bool) error {
	if asJSON {
		return writeJSONLines(w, rows)
	}
	for _, r := range rows {
		src := r.Source
		if src == "" {
			src = "-"
		}
		fmt.Fprintf(w, "%s  n=%d  sum=%g  %s\n", src, r.N, r.Sum, r.Unit)
	}
	return nil
}

func shortKind(k string) string {
	for _, p := range []string{
		"HKQuantityTypeIdentifier",
		"HKCategoryTypeIdentifier",
		"HKWorkoutActivityType",
		"HKCorrelationTypeIdentifier",
	} {
		if strings.HasPrefix(k, p) {
			return k[len(p):]
		}
	}
	return k
}
