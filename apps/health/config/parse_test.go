package config

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestPauseThreshold(t *testing.T) {
	ResetForTest()
	if PauseThreshold() != 60*time.Second {
		t.Fatalf("default %s", PauseThreshold())
	}
}

func TestPauseThresholdIgnoresEnv(t *testing.T) {
	t.Setenv("ARCHIVE_PAUSE_THRESHOLD_S", "999")
	ResetForTest()
	if PauseThreshold() != 60*time.Second {
		t.Fatalf("env must not override: %s", PauseThreshold())
	}
}

func TestReadTOMLIgnoresCWD(t *testing.T) {
	cwd, err := os.Getwd()
	if err != nil {
		t.Fatal(err)
	}
	dir := t.TempDir()
	if err := os.Mkdir(filepath.Join(dir, "config"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "config", "projection.toml"), []byte("pause_threshold_s = 7\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.Chdir(dir); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = os.Chdir(cwd) })
	ResetForTest()
	if PauseThreshold() != 60*time.Second {
		t.Fatalf("CWD must not override: %s", PauseThreshold())
	}
}

func TestReadTOMLUsesArchiveRoot(t *testing.T) {
	dir := t.TempDir()
	if err := os.Mkdir(filepath.Join(dir, "config"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "config", "projection.toml"), []byte("pause_threshold_s = 42\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	ResetForTest()
	SetRoot(dir)
	t.Cleanup(ResetForTest)
	if PauseThreshold() != 42*time.Second {
		t.Fatalf("archive root override: %s", PauseThreshold())
	}
}

func TestSourceRanks(t *testing.T) {
	ResetForTest()
	m := SourceRanks()
	if m["Apple Watch"] != 10 || m["iPhone"] != 20 || m["Pace"] != 30 || m["Mindful"] != 50 {
		t.Fatalf("%v", m)
	}
	m["Apple Watch"] = 999
	if SourceRank("Apple Watch") != 10 {
		t.Fatal("SourceRanks must return a copy")
	}
	if SourceRank("not-a-source") != rankUnlisted {
		t.Fatalf("unlisted=%d", SourceRank("not-a-source"))
	}
}

func TestTOMLReader(t *testing.T) {
	tests := []struct {
		name  string
		check func(*testing.T)
	}{
		{
			name: "inline comment after quoted value",
			check: func(t *testing.T) {
				k, v, ok := splitKV(stripInlineComment(`display = "Steps" # trailing`))
				if !ok || k != "display" || v != "Steps" {
					t.Fatalf("%q %q %v", k, v, ok)
				}
				if parsePauseThreshold("pause_threshold_s = 12 # seconds\n", 1) != 12 {
					t.Fatal("pause")
				}
			},
		},
		{
			name: "hash inside quoted string preserved",
			check: func(t *testing.T) {
				k, v, ok := splitKV(stripInlineComment(`display = "A # B" # trailing`))
				if !ok || k != "display" || v != "A # B" {
					t.Fatalf("%q %q %v", k, v, ok)
				}
			},
		},
		{
			name: "quoted keys with spaces",
			check: func(t *testing.T) {
				m := parseSources(`[sources]
"Apple Watch" = 10   # on-wrist
"iPhone" = 20
`)
				if m["Apple Watch"] != 10 || m["iPhone"] != 20 {
					t.Fatalf("%v", m)
				}
			},
		},
	}
	for _, tc := range tests {
		t.Run(tc.name, tc.check)
	}
}
