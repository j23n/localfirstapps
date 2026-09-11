package main

import (
	"strings"
	"testing"
)

func TestServeIsNotAProductCommand(t *testing.T) {
	code, _, errb := runCmd(t, "serve", "-addr", "127.0.0.1:8080")
	if code == 0 {
		t.Fatal("serve must not be a product command")
	}
	if !strings.Contains(errb, "unknown command") {
		t.Fatalf("err=%s", errb)
	}
}
