// Package event defines the append-only log record.
package event

import (
	"bytes"
	"encoding/json"
	"fmt"
	"regexp"
	"strings"
	"time"
)

const (
	TypeBlobImport  = "blob_import"
	TypeObservation = "observation"
	TypeMedStart    = "med_start"
	TypeMedStop     = "med_stop"
	TypeMedEvent    = "med_event"
	TypeMeditation  = "meditation"
	TypeNote        = "note"
	TypeExtraction  = "extraction"
	TypeSupersede   = "supersede"
	TypeRetract     = "retract"
)

// TSFormat is the canonical UTC timestamp stored in the log.
const TSFormat = "2006-01-02T15:04:05.000000000Z"

// Event is one NDJSON line. Field order matches the on-disk format.
type Event struct {
	ID   string          `json:"id"`
	TS   string          `json:"ts"`
	Dev  string          `json:"dev"`
	Type string          `json:"type"`
	Body json.RawMessage `json:"body"`
}

var (
	knownTypes = map[string]bool{
		TypeBlobImport:  true,
		TypeObservation: true,
		TypeMedStart:    true,
		TypeMedStop:     true,
		TypeMedEvent:    true,
		TypeMeditation:  true,
		TypeNote:        true,
		TypeExtraction:  true,
		TypeSupersede:   true,
		TypeRetract:     true,
	}
	deviceName = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._-]*$`)
)

// KnownType reports whether t is an allowed event type.
func KnownType(t string) bool { return knownTypes[t] }

// ValidDevice reports whether name is a safe log directory component.
func ValidDevice(name string) bool { return deviceName.MatchString(name) }

// Validate checks fields required to append.
func (e Event) Validate() error {
	if e.ID == "" {
		return fmt.Errorf("missing id")
	}
	if e.TS == "" {
		return fmt.Errorf("missing ts")
	}
	if len(e.TS) != len(TSFormat) {
		return fmt.Errorf("ts must be UTC with 9 fractional digits")
	}
	t, err := time.Parse(time.RFC3339Nano, e.TS)
	if err != nil {
		return fmt.Errorf("ts: %w", err)
	}
	if t.UTC().Format(TSFormat) != e.TS {
		return fmt.Errorf("ts must be UTC with 9 fractional digits")
	}
	if !ValidDevice(e.Dev) {
		return fmt.Errorf("invalid device %q", e.Dev)
	}
	if !KnownType(e.Type) {
		return fmt.Errorf("unknown type %q", e.Type)
	}
	if len(e.Body) == 0 {
		return fmt.Errorf("missing body")
	}
	if !json.Valid(e.Body) {
		return fmt.Errorf("body is not JSON")
	}
	trim := bytes.TrimSpace(e.Body)
	if len(trim) == 0 || trim[0] != '{' {
		return fmt.Errorf("body must be a JSON object")
	}
	if e.Type == TypeSupersede || e.Type == TypeRetract {
		var body struct {
			Target string `json:"target"`
		}
		if err := json.Unmarshal(e.Body, &body); err != nil {
			return err
		}
		if body.Target == "" {
			return fmt.Errorf("%s requires body.target", e.Type)
		}
	}
	return nil
}

// Month is the YYYY-MM partition for this event.
func (e Event) Month() string {
	if len(e.TS) >= 7 {
		return e.TS[:7]
	}
	return ""
}

// BlobImport is the decoded body of a blob_import event.
type BlobImport struct {
	SHA256 string
	Size   int64
	Name   string
	Kind   string
}

// ParseBlobImport reads a blob_import body. The written fields are
// sha256, size, name, kind. The aliases blob and orig_filename (docs/plan.md,
// event catalogue) are accepted when reading so hand-written lines resolve.
func ParseBlobImport(body json.RawMessage) (BlobImport, error) {
	var raw struct {
		SHA256       string `json:"sha256"`
		Blob         string `json:"blob"`
		Size         int64  `json:"size"`
		Name         string `json:"name"`
		OrigFilename string `json:"orig_filename"`
		Kind         string `json:"kind"`
	}
	if err := json.Unmarshal(body, &raw); err != nil {
		return BlobImport{}, err
	}
	b := BlobImport{
		SHA256: strings.ToLower(raw.SHA256),
		Size:   raw.Size,
		Name:   raw.Name,
		Kind:   raw.Kind,
	}
	if b.SHA256 == "" {
		b.SHA256 = strings.ToLower(raw.Blob)
	}
	if b.Name == "" {
		b.Name = raw.OrigFilename
	}
	return b, nil
}

// BlobSHA256 returns the content hash from body.sha256 or body.blob.
func (e Event) BlobSHA256() (string, bool) {
	b, err := ParseBlobImport(e.Body)
	if err != nil || b.SHA256 == "" {
		return "", false
	}
	return b.SHA256, true
}

// Target returns body.target for supersede/retract events.
func (e Event) Target() (string, bool) {
	var body struct {
		Target string `json:"target"`
	}
	if err := json.Unmarshal(e.Body, &body); err != nil || body.Target == "" {
		return "", false
	}
	return body.Target, true
}

// MarshalLine returns one compact NDJSON line including the trailing newline.
func (e Event) MarshalLine() ([]byte, error) {
	var buf bytes.Buffer
	enc := json.NewEncoder(&buf)
	enc.SetEscapeHTML(false)
	if err := enc.Encode(e); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}

// Less reports whether e sorts before o by (ts, id).
func (e Event) Less(o Event) bool {
	if e.TS != o.TS {
		return e.TS < o.TS
	}
	return e.ID < o.ID
}

// NowUTC returns the canonical timestamp for a newly appended event.
func NowUTC() string {
	return time.Now().UTC().Format(TSFormat)
}
