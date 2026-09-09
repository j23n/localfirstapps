package uuid

import (
	"encoding/hex"
	"strings"
	"testing"
	"time"
)

func TestNewV7(t *testing.T) {
	s, err := NewV7()
	if err != nil {
		t.Fatal(err)
	}
	parts := strings.Split(s, "-")
	if len(parts) != 5 || len(parts[0]) != 8 || len(parts[1]) != 4 || len(parts[2]) != 4 || len(parts[3]) != 4 || len(parts[4]) != 12 {
		t.Fatalf("format: %q", s)
	}
	raw, err := hex.DecodeString(strings.ReplaceAll(s, "-", ""))
	if err != nil {
		t.Fatal(err)
	}
	if len(raw) != 16 {
		t.Fatalf("len=%d", len(raw))
	}
	if ver := raw[6] >> 4; ver != 7 {
		t.Fatalf("version=%d", ver)
	}
	if v := raw[8] >> 6; v != 2 {
		t.Fatalf("variant=%d", v)
	}
	ms := uint64(raw[0])<<40 | uint64(raw[1])<<32 | uint64(raw[2])<<24 | uint64(raw[3])<<16 | uint64(raw[4])<<8 | uint64(raw[5])
	now := uint64(time.Now().UTC().UnixMilli())
	if ms+5000 < now || now+5000 < ms {
		t.Fatalf("timestamp %d far from now %d", ms, now)
	}
}

func TestNewV7Unique(t *testing.T) {
	a, err := NewV7()
	if err != nil {
		t.Fatal(err)
	}
	b, err := NewV7()
	if err != nil {
		t.Fatal(err)
	}
	if a == b {
		t.Fatal("duplicate")
	}
}
