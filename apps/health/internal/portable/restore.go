package portable

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"

	"archive/internal/blobs"
	"archive/internal/event"
	"archive/internal/log"
)

// Restore replays a portable export into destRoot, keeping event ids.
// Blobs are hashed into the store; events are appended as written.
// This is for round-trip tests and recovery drills, not a repair tool.
func Restore(exportDir, destRoot string) error {
	listed, err := blobs.List(exportDir)
	if err != nil {
		return err
	}
	for _, b := range listed {
		f, err := os.Open(b.Path)
		if err != nil {
			return err
		}
		hash, _, _, err := blobs.Put(destRoot, f)
		f.Close()
		if err != nil {
			return err
		}
		if hash != b.SHA256 {
			return fmt.Errorf("restore: blob %s hashed to %s", b.SHA256, hash)
		}
	}

	have := map[string]bool{}
	existing, err := log.ReadAll(destRoot)
	if err != nil {
		return err
	}
	for _, ev := range existing {
		have[ev.ID] = true
	}

	f, err := os.Open(filepath.Join(exportDir, "events.ndjson"))
	if err != nil {
		return err
	}
	defer f.Close()
	sc := bufio.NewScanner(f)
	sc.Buffer(make([]byte, 0, 64*1024), 8*1024*1024)
	lineNo := 0
	for sc.Scan() {
		lineNo++
		line := bytes.TrimSpace(sc.Bytes())
		if len(line) == 0 {
			continue
		}
		var ev event.Event
		if err := json.Unmarshal(line, &ev); err != nil {
			return fmt.Errorf("events.ndjson:%d: %w", lineNo, err)
		}
		if have[ev.ID] {
			continue
		}
		if err := log.Append(destRoot, ev); err != nil {
			return fmt.Errorf("events.ndjson:%d: %w", lineNo, err)
		}
		have[ev.ID] = true
	}
	return sc.Err()
}
