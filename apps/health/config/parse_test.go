package config

import (
	"os"
	"path/filepath"
	"reflect"
	"testing"
	"time"
)

func TestUnknownKindFallback(t *testing.T) {
	ResetForTest()
	s := Lookup("HKQuantityTypeIdentifierNotARealType")
	if s.Domain != DomainOther || s.Aggregate != "mean" || !s.Fallback {
		t.Fatalf("fallback=%+v", s)
	}
	if s.Display == "" {
		t.Fatal("display")
	}
	// must not panic
	_ = Lookup("")
}

func TestKnownKindFromTOML(t *testing.T) {
	ResetForTest()
	s := Lookup("HKQuantityTypeIdentifierStepCount")
	if s.Fallback {
		t.Fatal("steps should be configured")
	}
	if s.Domain != DomainSports || s.Aggregate != "sum" || s.Display != "Steps" {
		t.Fatalf("steps=%+v", s)
	}
}

func TestFavourites(t *testing.T) {
	ResetForTest()
	f := Favourites()
	if len(f) == 0 {
		t.Fatal("expected favourites")
	}
	for _, k := range f {
		if Lookup(k).Fallback && k != "" {
			t.Fatalf("favourite %s missing from kinds.toml", k)
		}
	}
	if !FavouriteWorkouts() {
		t.Fatal("expected blanket workout favourite")
	}
}

func TestIsWorkoutKind(t *testing.T) {
	if !IsWorkoutKind("HKWorkoutActivityTypeHiking") || IsWorkoutKind("HKQuantityTypeIdentifierStepCount") {
		t.Fatal("IsWorkoutKind")
	}
}

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

func TestInvalidDomainAndAggFallBack(t *testing.T) {
	if normalizeDomain("wellness") != DomainOther {
		t.Fatal("domain")
	}
	if normalizeAgg("median") != "mean" {
		t.Fatal("agg")
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
				kinds = map[string]Spec{}
				parseKinds("[HKQuantityTypeIdentifierStepCount]\ndisplay = \"Steps\" # trailing\ndomain = \"sports\"\n")
				sp := kinds["HKQuantityTypeIdentifierStepCount"]
				if sp.Display != "Steps" || sp.Domain != DomainSports {
					t.Fatalf("%+v", sp)
				}
				if parsePauseThreshold("pause_threshold_s = 12 # seconds\n", 1) != 12 {
					t.Fatal("pause")
				}
			},
		},
		{
			name: "section header with trailing comment",
			check: func(t *testing.T) {
				kinds = map[string]Spec{}
				parseKinds("[HKQuantityTypeIdentifierStepCount] # note\ndisplay = \"Steps\"\n")
				if _, ok := kinds["HKQuantityTypeIdentifierStepCount"]; !ok {
					t.Fatalf("%v", kinds)
				}
			},
		},
		{
			name: "single-line array",
			check: func(t *testing.T) {
				got := parseFavourites(`favourites = ["a", "b",]`)
				if !reflect.DeepEqual(got, []string{"a", "b"}) {
					t.Fatalf("%v", got)
				}
			},
		},
		{
			name: "multi-line array with comments between entries",
			check: func(t *testing.T) {
				got := parseFavourites(`favourites = [
  "a", # one
  "b",
]`)
				if !reflect.DeepEqual(got, []string{"a", "b"}) {
					t.Fatalf("%v", got)
				}
			},
		},
		{
			name: "hash inside quoted string preserved",
			check: func(t *testing.T) {
				kinds = map[string]Spec{}
				parseKinds("[Foo]\ndisplay = \"A # B\" # trailing\n")
				if kinds["Foo"].Display != "A # B" {
					t.Fatalf("%q", kinds["Foo"].Display)
				}
				got := parseFavourites(`favourites = ["foo#bar"]`)
				if !reflect.DeepEqual(got, []string{"foo#bar"}) {
					t.Fatalf("%v", got)
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
