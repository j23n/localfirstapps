// Package apple streams Apple Health export.xml without loading the document.
package apple

import (
	"crypto/sha256"
	"encoding/hex"
	"io"
	"sort"
	"strings"
	"time"
)

// KindApple is the blob_import body.kind for an Apple Health export.
const KindApple = "apple"

// AppleTimeLayout is the timestamp format used in export.xml attributes.
const AppleTimeLayout = "2006-01-02 15:04:05 -0700"

// Header is the document-level metadata (Me, ExportDate, locale).
type Header struct {
	Locale     string
	ExportDate string
	Me         map[string]string
}

// Meta is one MetadataEntry.
type Meta struct {
	Key   string `json:"key"`
	Value string `json:"value"`
}

// BPM is one InstantaneousBeatsPerMinute child of an HRV record.
type BPM struct {
	BPM  string `json:"bpm"`
	Time string `json:"time"`
}

// Observation is one top-level Record. kind is the Apple type identifier verbatim.
type Observation struct {
	DedupKey      string `json:"dedup_key"`
	Kind          string `json:"kind"`
	Source        string `json:"source"`
	SourceVersion string `json:"source_version,omitempty"`
	Device        string `json:"device,omitempty"`
	Start         string `json:"start_ts"`
	End           string `json:"end_ts"`
	StartOffset   string `json:"start_offset"`
	EndOffset     string `json:"end_offset"`
	Unit          string `json:"unit,omitempty"`
	Value         string `json:"value,omitempty"`
	Metadata      []Meta `json:"metadata,omitempty"`
	HRV           []BPM  `json:"hrv,omitempty"`
}

// Episode is a Workout, Correlation, ActivitySummary, or unknown top-level element.
type Episode struct {
	DedupKey    string         `json:"dedup_key"`
	Kind        string         `json:"kind"`
	Source      string         `json:"source,omitempty"`
	Start       string         `json:"start_ts"`
	End         string         `json:"end_ts"`
	StartOffset string         `json:"start_offset,omitempty"`
	EndOffset   string         `json:"end_offset,omitempty"`
	Body        map[string]any `json:"body"`
}

// Attachment is a zip member we do not decode (GPX routes, ECG CSVs).
type Attachment struct {
	Path string `json:"path"`
	Size int64  `json:"size"`
}

// DedupKey hashes type, sourceName, startDate, endDate, value, and sorted
// MetadataEntry (key, value) pairs. Unit is not part of the key. The key
// identifies a GROUP, not a unique row: identical Apple records share a key
// and the projector inserts the count difference against what is already in
// the database.
func DedupKey(typ, sourceName, startDate, endDate, value string, meta []Meta) string {
	h := sha256.New()
	io.WriteString(h, typ)
	h.Write([]byte{0})
	io.WriteString(h, sourceName)
	h.Write([]byte{0})
	io.WriteString(h, startDate)
	h.Write([]byte{0})
	io.WriteString(h, endDate)
	h.Write([]byte{0})
	io.WriteString(h, value)
	pairs := append([]Meta(nil), meta...)
	sort.Slice(pairs, func(i, j int) bool {
		if pairs[i].Key != pairs[j].Key {
			return pairs[i].Key < pairs[j].Key
		}
		return pairs[i].Value < pairs[j].Value
	})
	for _, p := range pairs {
		h.Write([]byte{0})
		io.WriteString(h, p.Key)
		h.Write([]byte{0})
		io.WriteString(h, p.Value)
	}
	return hex.EncodeToString(h.Sum(nil))
}

// ParseTime converts an Apple timestamp to UTC and the original offset (e.g. "-0800").
func ParseTime(s string) (utc, offset string, err error) {
	s = strings.TrimSpace(s)
	if s == "" {
		return "", "", errEmptyTime
	}
	t, err := time.Parse(AppleTimeLayout, s)
	if err != nil {
		t, err = time.Parse(time.RFC3339Nano, s)
		if err != nil {
			t, err = time.Parse(time.RFC3339, s)
			if err != nil {
				return "", "", err
			}
		}
	}
	_, offSec := t.Zone()
	return t.UTC().Format("2006-01-02T15:04:05.000000000Z"), formatOffset(offSec), nil
}

var errEmptyTime = errString("empty timestamp")

type errString string

func (e errString) Error() string { return string(e) }

func formatOffset(sec int) string {
	sign := '+'
	if sec < 0 {
		sign = '-'
		sec = -sec
	}
	h := sec / 3600
	m := (sec % 3600) / 60
	return string([]byte{byte(sign), '0' + byte(h/10), '0' + byte(h%10), '0' + byte(m/10), '0' + byte(m%10)})
}
