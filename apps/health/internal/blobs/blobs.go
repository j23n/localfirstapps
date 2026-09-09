// Package blobs is a content-addressed SHA-256 store.
//
// Layout: blobs/sha256/ab/cd/<64-hex>
package blobs

import (
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

func validSHA256(h string) bool {
	if len(h) != 64 {
		return false
	}
	for _, c := range h {
		if (c < '0' || c > '9') && (c < 'a' || c > 'f') && (c < 'A' || c > 'F') {
			return false
		}
	}
	return true
}

// Path returns the store path for a hex SHA-256 digest.
func Path(root, hash string) (string, error) {
	h := strings.ToLower(hash)
	if !validSHA256(h) {
		return "", fmt.Errorf("sha256 must be 64 hex chars")
	}
	return filepath.Join(root, "blobs", "sha256", h[:2], h[2:4], h), nil
}

// Exists reports whether the blob is already stored.
func Exists(root, hash string) (bool, error) {
	p, err := Path(root, hash)
	if err != nil {
		return false, err
	}
	_, err = os.Stat(p)
	if os.IsNotExist(err) {
		return false, nil
	}
	return err == nil, err
}

// Put writes r into the store. existed is true when the digest was already present.
func Put(root string, r io.Reader) (hash string, size int64, existed bool, err error) {
	tmpDir := filepath.Join(root, "blobs", "tmp")
	if err := os.MkdirAll(tmpDir, 0o700); err != nil {
		return "", 0, false, err
	}
	tmp, err := os.CreateTemp(tmpDir, "put-")
	if err != nil {
		return "", 0, false, err
	}
	tmpName := tmp.Name()
	h := sha256.New()
	n, copyErr := io.Copy(io.MultiWriter(h, tmp), r)
	syncErr := tmp.Sync()
	closeErr := tmp.Close()
	if copyErr != nil {
		os.Remove(tmpName)
		return "", 0, false, copyErr
	}
	if syncErr != nil {
		os.Remove(tmpName)
		return "", 0, false, syncErr
	}
	if closeErr != nil {
		os.Remove(tmpName)
		return "", 0, false, closeErr
	}
	sum := hex.EncodeToString(h.Sum(nil))
	dest, err := Path(root, sum)
	if err != nil {
		os.Remove(tmpName)
		return "", 0, false, err
	}
	if _, err := os.Stat(dest); err == nil {
		os.Remove(tmpName)
		return sum, n, true, nil
	} else if err != nil && !os.IsNotExist(err) {
		os.Remove(tmpName)
		return "", 0, false, err
	}
	if err := os.MkdirAll(filepath.Dir(dest), 0o700); err != nil {
		os.Remove(tmpName)
		return "", 0, false, err
	}
	if err := os.Rename(tmpName, dest); err != nil {
		os.Remove(tmpName)
		return "", 0, false, err
	}
	return sum, n, false, nil
}

// Open returns a reader for a stored blob.
func Open(root, hash string) (*os.File, error) {
	p, err := Path(root, hash)
	if err != nil {
		return nil, err
	}
	return os.Open(p)
}

// Info is one stored blob.
type Info struct {
	SHA256 string
	Size   int64
	Path   string
}

// List walks blobs/sha256/ab/cd/<hash>, skipping tmp.
func List(root string) ([]Info, error) {
	base := filepath.Join(root, "blobs", "sha256")
	if _, err := os.Stat(base); os.IsNotExist(err) {
		return nil, nil
	} else if err != nil {
		return nil, err
	}
	var out []Info
	err := filepath.WalkDir(base, func(path string, d fs.DirEntry, err error) error {
		if err != nil {
			return err
		}
		if d.IsDir() {
			return nil
		}
		name := d.Name()
		if !validSHA256(name) {
			return nil
		}
		info, err := d.Info()
		if err != nil {
			return err
		}
		out = append(out, Info{SHA256: strings.ToLower(name), Size: info.Size(), Path: path})
		return nil
	})
	if err != nil {
		return nil, err
	}
	sort.Slice(out, func(i, j int) bool { return out[i].SHA256 < out[j].SHA256 })
	return out, nil
}

// HashFile streams path and returns its SHA-256 hex digest and size.
func HashFile(path string) (string, int64, error) {
	f, err := os.Open(path)
	if err != nil {
		return "", 0, err
	}
	defer f.Close()
	h := sha256.New()
	n, err := io.Copy(h, f)
	if err != nil {
		return "", n, err
	}
	return hex.EncodeToString(h.Sum(nil)), n, nil
}

// Verify reports whether the file at the content-addressed path hashes to hash.
func Verify(root, hash string) error {
	p, err := Path(root, hash)
	if err != nil {
		return err
	}
	got, _, err := HashFile(p)
	if err != nil {
		return err
	}
	if got != strings.ToLower(hash) {
		return fmt.Errorf("blob %s: content hashes to %s", hash, got)
	}
	return nil
}
