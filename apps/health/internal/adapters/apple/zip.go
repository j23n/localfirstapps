package apple

import (
	"archive/zip"
	"bufio"
	"bytes"
	"fmt"
	"io"
	"os"
	"strings"
)

const (
	healthDataLookBytes = 1 << 20 // 1 MiB
	healthDataLookLines = 4096
)

// IsExport reports whether path is an Apple Health export (zip with export.xml, or export.xml itself).
func IsExport(path string) bool {
	switch exportKind(path) {
	case "xml":
		ok, _ := fileLooksLikeHealthData(path)
		return ok
	case "zip":
		r, err := zip.OpenReader(path)
		if err != nil {
			return false
		}
		defer r.Close()
		return findExportXML(r) != nil
	default:
		return false
	}
}

// exportKind classifies path as "xml", "zip", or "". Suffixes are a fast path;
// extension-less blob hashes are sniffed by magic / HealthData content.
func exportKind(path string) string {
	lower := strings.ToLower(path)
	if strings.HasSuffix(lower, ".xml") {
		return "xml"
	}
	if strings.HasSuffix(lower, ".zip") {
		return "zip"
	}
	f, err := os.Open(path)
	if err != nil {
		return ""
	}
	var magic [4]byte
	n, _ := io.ReadFull(f, magic[:])
	f.Close()
	if n >= 4 && magic[0] == 'P' && magic[1] == 'K' && magic[2] == 0x03 && magic[3] == 0x04 {
		return "zip"
	}
	ok, _ := fileLooksLikeHealthData(path)
	if ok {
		return "xml"
	}
	return ""
}

func fileLooksLikeHealthData(path string) (bool, error) {
	f, err := os.Open(path)
	if err != nil {
		return false, err
	}
	defer f.Close()
	br := bufio.NewReader(io.LimitReader(f, healthDataLookBytes))
	for n := 0; n < healthDataLookLines; n++ {
		line, isPrefix, err := br.ReadLine()
		if bytes.Contains(line, []byte("<HealthData")) {
			return true, nil
		}
		for isPrefix {
			_, isPrefix, err = br.ReadLine()
			if err != nil {
				break
			}
		}
		if err == io.EOF {
			return false, nil
		}
		if err != nil {
			return false, err
		}
	}
	return false, nil
}

func findExportXML(r *zip.ReadCloser) *zip.File {
	var fallback *zip.File
	for i := range r.File {
		name := r.File[i].Name
		base := name
		if i := strings.LastIndex(name, "/"); i >= 0 {
			base = name[i+1:]
		}
		if base == "export_cda.xml" {
			continue
		}
		if base == "export.xml" {
			return r.File[i]
		}
		if strings.HasSuffix(name, "export.xml") {
			fallback = r.File[i]
		}
	}
	return fallback
}

// OpenXML returns a reader for export.xml inside a zip, or the file itself if it is XML.
// The caller must Close the result (which also closes an underlying zip).
func OpenXML(path string) (io.ReadCloser, error) {
	switch exportKind(path) {
	case "xml":
		return os.Open(path)
	case "zip":
		zr, err := zip.OpenReader(path)
		if err != nil {
			return nil, err
		}
		zf := findExportXML(zr)
		if zf == nil {
			zr.Close()
			return nil, fmt.Errorf("apple: no export.xml in %s", path)
		}
		rc, err := zf.Open()
		if err != nil {
			zr.Close()
			return nil, err
		}
		return &zipXML{ReadCloser: rc, zip: zr}, nil
	default:
		return nil, fmt.Errorf("apple: not an export: %s", path)
	}
}

type zipXML struct {
	io.ReadCloser
	zip *zip.ReadCloser
}

func (z *zipXML) Close() error {
	err := z.ReadCloser.Close()
	err2 := z.zip.Close()
	if err != nil {
		return err
	}
	return err2
}

// ListAttachments names GPX routes and ECG CSVs inside the zip (unparsed).
func ListAttachments(path string) ([]Attachment, error) {
	r, err := zip.OpenReader(path)
	if err != nil {
		return nil, nil // not a zip — no attachments
	}
	defer r.Close()
	var out []Attachment
	for i := range r.File {
		name := r.File[i].Name
		lower := strings.ToLower(name)
		if (strings.Contains(lower, "workout-routes/") && strings.HasSuffix(lower, ".gpx")) ||
			(strings.Contains(lower, "electrocardiograms/") && strings.HasSuffix(lower, ".csv")) {
			out = append(out, Attachment{Path: name, Size: int64(r.File[i].UncompressedSize64)})
		}
	}
	return out, nil
}

// OpenMember opens a zip member whose path equals ref or ends with "/"+ref (Apple FileReference).
// The caller must Close the result (which also closes the zip).
func OpenMember(zipPath, ref string) (io.ReadCloser, error) {
	zr, err := zip.OpenReader(zipPath)
	if err != nil {
		return nil, err
	}
	ref = strings.TrimPrefix(strings.ReplaceAll(ref, "\\", "/"), "/")
	var match *zip.File
	for i := range zr.File {
		name := strings.ReplaceAll(zr.File[i].Name, "\\", "/")
		if ref == "" {
			continue
		}
		if name == ref || strings.HasSuffix(name, "/"+ref) {
			match = zr.File[i]
			break
		}
	}
	if match == nil {
		zr.Close()
		return nil, fmt.Errorf("apple: no zip member matching %s", ref)
	}
	rc, err := match.Open()
	if err != nil {
		zr.Close()
		return nil, err
	}
	return &zipXML{ReadCloser: rc, zip: zr}, nil
}
