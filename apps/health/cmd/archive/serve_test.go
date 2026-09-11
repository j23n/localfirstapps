package main

import (
	"strings"
	"testing"
)

func TestServeRefusesNonLocalhost(t *testing.T) {
	code, _, errb := runCmd(t, "serve", "-addr", "0.0.0.0:8080")
	if code == 0 {
		t.Fatal("serve must refuse 0.0.0.0")
	}
	if !strings.Contains(errb, "127.0.0.1") {
		t.Fatalf("err=%s", errb)
	}
}
