// Package log appends and reads the NDJSON event log.
//
// Files are append-only. Never rewrite, edit, or delete a line.
package log

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"archive/internal/event"
)

// TornTailError is a final line that lacks a trailing newline and is not valid JSON.
// The append-only log must not be rewritten; callers should report, not repair.
type TornTailError struct {
	Path   string
	Offset int64
	Err    error
}

func (e *TornTailError) Error() string {
	return fmt.Sprintf("torn tail %s at offset %d: %v", e.Path, e.Offset, e.Err)
}

func (e *TornTailError) Unwrap() error { return e.Err }

// Report contains events recovered from all complete lines and diagnostics for
// ignored torn tails.
type Report struct {
	Events    []event.Event
	TornTails []TornTailError
}

// Append writes one event line to log/<dev>/YYYY-MM.ndjson.
func Append(root string, ev event.Event) error {
	if err := ev.Validate(); err != nil {
		return err
	}
	month := ev.Month()
	if month == "" {
		return fmt.Errorf("event ts has no month")
	}
	dir := filepath.Join(root, "log", ev.Dev)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	line, err := ev.MarshalLine()
	if err != nil {
		return err
	}
	path := filepath.Join(dir, month+".ndjson")
	_, statErr := os.Stat(path)
	created := os.IsNotExist(statErr)
	if statErr != nil && !created {
		return statErr
	}
	f, err := os.OpenFile(path, os.O_APPEND|os.O_CREATE|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	_, werr := f.Write(line)
	serr := f.Sync()
	cerr := f.Close()
	if werr != nil {
		return werr
	}
	if serr != nil {
		return serr
	}
	if cerr != nil {
		return cerr
	}
	if created {
		return syncDir(dir)
	}
	return nil
}

func syncDir(dir string) error {
	d, err := os.Open(dir)
	if err != nil {
		return err
	}
	defer d.Close()
	return d.Sync()
}

// ReadAll walks log/*/*.ndjson and returns events sorted by (ts, id).
//
// Use ReadReport when complete events must remain available in the presence
// of a torn tail. ReadAll preserves its strict behavior for callers that must
// not proceed without inspecting that diagnostic.
func ReadAll(root string) ([]event.Event, error) {
	rep, err := ReadReport(root)
	if err != nil {
		return nil, err
	}
	if len(rep.TornTails) > 0 {
		return rep.Events, &rep.TornTails[0]
	}
	return rep.Events, nil
}

// ReadReport walks log/*/*.ndjson and returns complete events plus torn-tail
// diagnostics. Malformed newline-terminated content remains a hard error.
func ReadReport(root string) (Report, error) {
	dir := filepath.Join(root, "log")
	if _, err := os.Stat(dir); os.IsNotExist(err) {
		return Report{}, nil
	} else if err != nil {
		return Report{}, err
	}
	var rep Report
	err := filepath.WalkDir(dir, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}
		if !strings.HasSuffix(d.Name(), ".ndjson") {
			return nil
		}
		evs, torn, err := readFile(path)
		if err != nil {
			return err
		}
		rep.Events = append(rep.Events, evs...)
		if torn != nil {
			rep.TornTails = append(rep.TornTails, *torn)
		}
		return nil
	})
	if err != nil {
		return Report{}, err
	}
	sort.Slice(rep.Events, func(i, j int) bool { return rep.Events[i].Less(rep.Events[j]) })
	return rep, nil
}

func readFile(path string) ([]event.Event, *TornTailError, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, nil, err
	}
	defer f.Close()
	br := bufio.NewReaderSize(f, 64*1024)
	var out []event.Event
	var offset int64
	lineNo := 0
	for {
		line, err := br.ReadBytes('\n')
		if len(line) == 0 && err == io.EOF {
			break
		}
		if err != nil && err != io.EOF {
			return nil, nil, fmt.Errorf("%s: %w", path, err)
		}
		hasNL := len(line) > 0 && line[len(line)-1] == '\n'
		raw := bytes.TrimSpace(line)
		if len(raw) == 0 {
			offset += int64(len(line))
			if err == io.EOF {
				break
			}
			continue
		}
		lineNo++
		var ev event.Event
		if jerr := json.Unmarshal(raw, &ev); jerr != nil {
			if !hasNL && err == io.EOF {
				return out, &TornTailError{Path: path, Offset: offset, Err: jerr}, nil
			}
			return nil, nil, fmt.Errorf("%s:%d: %w", path, lineNo, jerr)
		}
		out = append(out, ev)
		offset += int64(len(line))
		if err == io.EOF {
			break
		}
	}
	return out, nil, nil
}

// HasBlobImport reports whether a blob_import for sha256 already exists.
func HasBlobImport(root, sha256 string) (bool, error) {
	evs, err := ReadAll(root)
	if err != nil {
		return false, err
	}
	want := strings.ToLower(sha256)
	for _, ev := range evs {
		if ev.Type != event.TypeBlobImport {
			continue
		}
		hash, ok := ev.BlobSHA256()
		if ok && hash == want {
			return true, nil
		}
	}
	return false, nil
}
