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
func ReadAll(root string) ([]event.Event, error) {
	dir := filepath.Join(root, "log")
	if _, err := os.Stat(dir); os.IsNotExist(err) {
		return nil, nil
	} else if err != nil {
		return nil, err
	}
	var out []event.Event
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
		evs, err := readFile(path)
		if err != nil {
			return err
		}
		out = append(out, evs...)
		return nil
	})
	if err != nil {
		return nil, err
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Less(out[j]) })
	return out, nil
}

func readFile(path string) ([]event.Event, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
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
			return nil, fmt.Errorf("%s: %w", path, err)
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
				return nil, &TornTailError{Path: path, Offset: offset, Err: jerr}
			}
			return nil, fmt.Errorf("%s:%d: %w", path, lineNo, jerr)
		}
		out = append(out, ev)
		offset += int64(len(line))
		if err == io.EOF {
			break
		}
	}
	return out, nil
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
