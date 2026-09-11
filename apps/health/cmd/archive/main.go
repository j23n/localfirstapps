// Command archive is the personal health archive CLI.
package main

import (
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"archive/config"
	"archive/internal/adapters/apple"
	"archive/internal/blobs"
	"archive/internal/event"
	"archive/internal/log"
	"archive/internal/portable"
	"archive/internal/projection"
	"archive/internal/uuid"
)

func main() {
	os.Exit(run(os.Stdout, os.Stderr, os.Args[1:]))
}

func run(stdout, stderr io.Writer, args []string) int {
	root := os.Getenv("ARCHIVE_ROOT")
	if root == "" {
		root = "archive"
	}
	if len(args) >= 2 && (args[0] == "-root" || args[0] == "--root") {
		root = args[1]
		args = args[2:]
	}
	config.SetRoot(root)
	if len(args) == 0 || args[0] == "-h" || args[0] == "--help" {
		usage(stderr)
		if len(args) == 0 {
			return 2
		}
		return 0
	}
	warnCloudSync(stderr, root)
	var err error
	switch args[0] {
	case "append":
		err = cmdAppend(stdout, root, args[1:])
	case "import":
		err = cmdImport(stdout, root, args[1:])
	case "rebuild":
		err = cmdRebuild(stdout, root, args[1:])
	case "query":
		err = cmdQuery(stdout, root, args[1:])
	case "observations":
		err = cmdObservations(stdout, stderr, root, args[1:])
	case "episodes":
		err = cmdEpisodes(stdout, stderr, root, args[1:])
	case "kinds":
		err = cmdKinds(stdout, root, args[1:])
	case "sources":
		err = cmdSources(stdout, root, args[1:])
	case "stats":
		err = cmdStats(stdout, root, args[1:])
	case "export":
		err = cmdExport(stdout, root, args[1:])
	case "fsck":
		err = cmdFsck(stdout, root, args[1:])
	default:
		fmt.Fprintf(stderr, "unknown command %q\n", args[0])
		usage(stderr)
		return 2
	}
	if err != nil {
		if err != errFsckFailed {
			fmt.Fprintln(stderr, err)
		}
		return 1
	}
	return 0
}

func usage(w io.Writer) {
	fmt.Fprint(w, `usage: archive [-root DIR] <command> [args]

commands:
  append   write an event to the log
  import   hash a file into blobs and record blob_import
  rebuild  reconstruct derived/archive.db from log + blobs
  query         read events (or observations/episodes via extra arg)
  observations  read projected records (-kind -source -on -from -to -sum -by-source -summary)
  episodes      read projected workouts/summaries
  kinds         list observation and episode kinds
  sources       list observation sourceNames
  stats         counts and observation date range
  export        write a portable copy of the archive to -out DIR
  fsck          verify blob hashes and references (read-only)

Local calendar dates (-on/-from/-to) use $ARCHIVE_TZ (IANA or +0200), else the process local zone.
The UTC window and offset are printed on stderr.

-root defaults to $ARCHIVE_ROOT or ./archive
`)
}

var (
	errFsckFailed = errors.New("fsck failed")
	cloudMarkers  = []string{"dropbox", "nextcloud", "onedrive", "icloud", "google drive", "googledrive"}
)

func warnCloudSync(stderr io.Writer, root string) {
	abs, err := filepath.Abs(root)
	if err != nil {
		return
	}
	lower := strings.ToLower(abs)
	for _, m := range cloudMarkers {
		if strings.Contains(lower, m) {
			fmt.Fprintf(stderr, "warning: archive root %s sits under a cloud-sync path (%s); keep the archive out of Dropbox, Nextcloud, OneDrive, iCloud, and Google Drive\n", abs, m)
			return
		}
	}
}

func cmdAppend(stdout io.Writer, root string, args []string) error {
	fs := flag.NewFlagSet("append", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	dev := fs.String("dev", "", "device name")
	typ := fs.String("type", "", "event type")
	body := fs.String("body", "{}", "JSON object body")
	if err := fs.Parse(args); err != nil {
		return fmt.Errorf("append: %w", err)
	}
	if *dev == "" || *typ == "" {
		return fmt.Errorf("append: -dev and -type are required")
	}
	id, err := uuid.NewV7()
	if err != nil {
		return err
	}
	ev := event.Event{
		ID:   id,
		TS:   event.NowUTC(),
		Dev:  *dev,
		Type: *typ,
		Body: json.RawMessage(*body),
	}
	if err := log.Append(root, ev); err != nil {
		return fmt.Errorf("append: %w", err)
	}
	line, err := ev.MarshalLine()
	if err != nil {
		return err
	}
	_, err = stdout.Write(line)
	return err
}

func cmdImport(stdout io.Writer, root string, args []string) error {
	fs := flag.NewFlagSet("import", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	dev := fs.String("dev", "", "device name")
	name := fs.String("name", "", "original file name (default: basename)")
	if err := fs.Parse(args); err != nil {
		return fmt.Errorf("import: %w", err)
	}
	if *dev == "" {
		return fmt.Errorf("import: -dev is required")
	}
	if fs.NArg() != 1 {
		return fmt.Errorf("import: expected one file")
	}
	src := fs.Arg(0)
	if *name == "" {
		*name = filepath.Base(src)
	}
	f, err := os.Open(src)
	if err != nil {
		return fmt.Errorf("import: %w", err)
	}
	hash, size, _, err := blobs.Put(root, f)
	f.Close()
	if err != nil {
		return fmt.Errorf("import: %w", err)
	}
	exists, err := log.HasBlobImport(root, hash)
	if err != nil {
		return err
	}
	if exists {
		fmt.Fprintf(stdout, "idempotent sha256=%s size=%d\n", hash, size)
		return nil
	}
	kind := ""
	if p, err := blobs.Path(root, hash); err == nil && apple.IsExport(p) {
		kind = apple.KindApple
	}
	body, err := json.Marshal(struct {
		SHA256 string `json:"sha256"`
		Size   int64  `json:"size"`
		Name   string `json:"name"`
		Kind   string `json:"kind,omitempty"`
	}{hash, size, *name, kind})
	if err != nil {
		return err
	}
	id, err := uuid.NewV7()
	if err != nil {
		return err
	}
	ev := event.Event{
		ID:   id,
		TS:   event.NowUTC(),
		Dev:  *dev,
		Type: event.TypeBlobImport,
		Body: body,
	}
	if err := log.Append(root, ev); err != nil {
		return fmt.Errorf("import: %w", err)
	}
	line, err := ev.MarshalLine()
	if err != nil {
		return err
	}
	_, err = stdout.Write(line)
	return err
}

func cmdRebuild(stdout io.Writer, root string, args []string) error {
	fs := flag.NewFlagSet("rebuild", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	if err := fs.Parse(args); err != nil {
		return fmt.Errorf("rebuild: %w", err)
	}
	if err := projection.Rebuild(root); err != nil {
		return fmt.Errorf("rebuild: %w", err)
	}
	fmt.Fprintln(stdout, projection.DBPath(root))
	return nil
}

func cmdQuery(stdout io.Writer, root string, args []string) error {
	fs := flag.NewFlagSet("query", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	table := fs.String("table", "events", "events, observations, or episodes")
	typ := fs.String("type", "", "filter by event type")
	kind := fs.String("kind", "", "filter observations/episodes by Apple type")
	dev := fs.String("dev", "", "filter by device")
	source := fs.String("source", "", "filter observations by sourceName")
	id := fs.String("id", "", "filter by id")
	current := fs.Bool("current", false, "omit superseded and retracted events")
	positional := ""
	if len(args) > 0 && (args[0] == "events" || args[0] == "observations" || args[0] == "episodes") {
		positional = args[0]
		args = args[1:]
	}
	if err := fs.Parse(args); err != nil {
		return fmt.Errorf("query: %w", err)
	}
	if positional != "" {
		*table = positional
	}
	db, err := projection.Open(root)
	if err != nil {
		return err
	}
	defer db.Close()
	switch *table {
	case "events", "":
		evs, err := projection.Query(db, projection.Filter{
			Type:    *typ,
			Dev:     *dev,
			ID:      *id,
			Current: *current,
		})
		if err != nil {
			return fmt.Errorf("query: %w", err)
		}
		for _, ev := range evs {
			line, err := ev.MarshalLine()
			if err != nil {
				return err
			}
			if _, err := stdout.Write(line); err != nil {
				return err
			}
		}
	case "observations":
		rows, err := projection.QueryObservations(db, *kind, *source)
		if err != nil {
			return fmt.Errorf("query: %w", err)
		}
		return writeJSONLines(stdout, rows)
	case "episodes":
		rows, err := projection.QueryEpisodes(db, *kind)
		if err != nil {
			return fmt.Errorf("query: %w", err)
		}
		return writeJSONLines(stdout, rows)
	default:
		return fmt.Errorf("query: unknown table %q", *table)
	}
	return nil
}

func cmdExport(stdout io.Writer, root string, args []string) error {
	fs := flag.NewFlagSet("export", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	out := fs.String("out", "", "destination directory")
	if err := fs.Parse(args); err != nil {
		return fmt.Errorf("export: %w", err)
	}
	if *out == "" {
		return fmt.Errorf("export: -out is required")
	}
	man, err := portable.Export(root, *out)
	if err != nil {
		return err
	}
	fmt.Fprintf(stdout, "exported %d events %d blobs -> %s\n", man.EventCount, man.BlobCount, *out)
	return nil
}

func cmdFsck(stdout io.Writer, root string, args []string) error {
	fs := flag.NewFlagSet("fsck", flag.ContinueOnError)
	fs.SetOutput(io.Discard)
	if err := fs.Parse(args); err != nil {
		return fmt.Errorf("fsck: %w", err)
	}
	rep, err := portable.Fsck(root)
	if err != nil {
		return fmt.Errorf("fsck: %w", err)
	}
	rep.Write(stdout)
	if !rep.OK() {
		return errFsckFailed
	}
	return nil
}

func writeJSONLines(w io.Writer, v any) error {
	enc := json.NewEncoder(w)
	enc.SetEscapeHTML(false)
	switch rows := v.(type) {
	case []projection.ObservationRow:
		for _, r := range rows {
			if err := enc.Encode(r); err != nil {
				return err
			}
		}
	case []projection.EpisodeRow:
		for _, r := range rows {
			if err := enc.Encode(r); err != nil {
				return err
			}
		}
	case []projection.SumRow:
		for _, r := range rows {
			if err := enc.Encode(r); err != nil {
				return err
			}
		}
	case []projection.KindName:
		for _, r := range rows {
			if err := enc.Encode(r); err != nil {
				return err
			}
		}
	case []projection.SourceName:
		for _, r := range rows {
			if err := enc.Encode(r); err != nil {
				return err
			}
		}
	default:
		return enc.Encode(v)
	}
	return nil
}
