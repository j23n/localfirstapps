// Package portable writes and checks a self-describing archive export.
//
// The export is readable without Go, SQLite, or this repository.
package portable

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"

	"archive/internal/blobs"
	"archive/internal/event"
	"archive/internal/log"
	"archive/internal/version"
)

// ArchiveVersion is the portable export format version.
const ArchiveVersion = 1

// Manifest describes one export. Only ExportedAt changes between
// otherwise identical exports of an unchanged archive.
type Manifest struct {
	ArchiveVersion int            `json:"archive_version"`
	ExportedAt     string         `json:"exported_at"`
	ToolVersion    string         `json:"tool_version"`
	EventCount     int            `json:"event_count"`
	BlobCount      int            `json:"blob_count"`
	Blobs          []ManifestBlob `json:"blobs"`
}

// ManifestBlob is one verified blob in the export.
type ManifestBlob struct {
	SHA256 string `json:"sha256"`
	Size   int64  `json:"size"`
}

// Export writes events.ndjson, blobs/, manifest.json, and README.md to outDir.
// Every blob is hashed while copied; a mismatch is an error and the
// destination file is removed. Blobs are never loaded into memory.
func Export(root, outDir string) (Manifest, error) {
	var zero Manifest
	absRoot, err := filepath.Abs(root)
	if err != nil {
		return zero, err
	}
	absOut, err := filepath.Abs(outDir)
	if err != nil {
		return zero, err
	}
	if absOut == absRoot || strings.HasPrefix(absOut, absRoot+string(os.PathSeparator)) {
		return zero, fmt.Errorf("export: -out must not be inside the archive root")
	}
	if absRoot == absOut || strings.HasPrefix(absRoot, absOut+string(os.PathSeparator)) {
		return zero, fmt.Errorf("export: -out must not be a parent of the archive root")
	}

	evs, err := log.ReadAll(root)
	if err != nil {
		return zero, err
	}
	listed, err := blobs.List(root)
	if err != nil {
		return zero, err
	}

	if err := os.MkdirAll(outDir, 0o700); err != nil {
		return zero, err
	}

	if err := writeEvents(filepath.Join(outDir, "events.ndjson"), evs); err != nil {
		return zero, err
	}

	manBlobs := make([]ManifestBlob, 0, len(listed))
	for _, b := range listed {
		dest, err := blobs.Path(outDir, b.SHA256)
		if err != nil {
			return zero, err
		}
		n, err := copyVerified(b.Path, dest, b.SHA256)
		if err != nil {
			return zero, err
		}
		manBlobs = append(manBlobs, ManifestBlob{SHA256: b.SHA256, Size: n})
	}

	if err := os.WriteFile(filepath.Join(outDir, "README.md"), []byte(readmeText), 0o600); err != nil {
		return zero, err
	}

	man := Manifest{
		ArchiveVersion: ArchiveVersion,
		ExportedAt:     event.NowUTC(),
		ToolVersion:    version.Version,
		EventCount:     len(evs),
		BlobCount:      len(manBlobs),
		Blobs:          manBlobs,
	}
	if err := writeManifest(filepath.Join(outDir, "manifest.json"), man); err != nil {
		return zero, err
	}
	return man, nil
}

func writeEvents(path string, evs []event.Event) error {
	f, err := os.OpenFile(path, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	for _, ev := range evs {
		line, err := ev.MarshalLine()
		if err != nil {
			f.Close()
			return err
		}
		if _, err := f.Write(line); err != nil {
			f.Close()
			return err
		}
	}
	if err := f.Sync(); err != nil {
		f.Close()
		return err
	}
	return f.Close()
}

func writeManifest(path string, man Manifest) error {
	f, err := os.OpenFile(path, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	enc := json.NewEncoder(f)
	enc.SetEscapeHTML(false)
	enc.SetIndent("", "  ")
	if err := enc.Encode(man); err != nil {
		f.Close()
		return err
	}
	if err := f.Sync(); err != nil {
		f.Close()
		return err
	}
	return f.Close()
}

// copyVerified streams src to dest while hashing. On mismatch dest is removed.
func copyVerified(src, dest, expect string) (int64, error) {
	in, err := os.Open(src)
	if err != nil {
		return 0, err
	}
	defer in.Close()
	if err := os.MkdirAll(filepath.Dir(dest), 0o700); err != nil {
		return 0, err
	}
	out, err := os.OpenFile(dest, os.O_CREATE|os.O_TRUNC|os.O_WRONLY, 0o600)
	if err != nil {
		return 0, err
	}
	h := sha256.New()
	n, copyErr := io.Copy(io.MultiWriter(out, h), in)
	syncErr := out.Sync()
	closeErr := out.Close()
	if copyErr != nil {
		os.Remove(dest)
		return n, copyErr
	}
	if syncErr != nil {
		os.Remove(dest)
		return n, syncErr
	}
	if closeErr != nil {
		os.Remove(dest)
		return n, closeErr
	}
	got := hex.EncodeToString(h.Sum(nil))
	want := strings.ToLower(expect)
	if got != want {
		os.Remove(dest)
		return n, fmt.Errorf("export: blob mismatch sha256=%s content hashes to %s — not copied", want, got)
	}
	return n, nil
}
