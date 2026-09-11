package projection

import (
	"database/sql"
	"fmt"
	"strings"
)

// ResolvedKind is a suffix/exact match plus which projection tables hold it.
type ResolvedKind struct {
	Kind         string
	Observations bool
	Episodes     bool
}

// ResolveKindInDB matches q against observation and episode kinds.
// prefer is "observations" or "episodes": that table is tried first so
// -kind Running stays unique on episodes (workout) vs observations (distance).
// If the preferred table has no match, the other table is searched (SleepAnalysis
// is an observation even when the command was episodes).
func ResolveKindInDB(db *sql.DB, q, prefer string) (ResolvedKind, error) {
	var zero ResolvedKind
	if q == "" {
		return zero, nil
	}
	obs, err := ObservationKindNames(db)
	if err != nil {
		return zero, err
	}
	eps, err := EpisodeKindNames(db)
	if err != nil {
		return zero, err
	}
	primary, secondary := obs, eps
	if prefer == "episodes" {
		primary, secondary = eps, obs
	}
	kind, err := ResolveKind(primary, q)
	if err != nil && !strings.Contains(err.Error(), "ambiguous") {
		kind, err = ResolveKind(secondary, q)
	}
	if err != nil {
		return zero, err
	}
	return ResolvedKind{
		Kind:         kind,
		Observations: contains(obs, kind),
		Episodes:     contains(eps, kind),
	}, nil
}

func contains(names []string, want string) bool {
	for _, n := range names {
		if n == want {
			return true
		}
	}
	return false
}

// ResolveKind matches q against names. Exact match wins; otherwise a unique
// suffix match (so StepCount resolves HKQuantityTypeIdentifierStepCount and
// SleepAnalysis resolves HKCategoryTypeIdentifierSleepAnalysis).
// Multiple suffix hits are an error — the caller must type more of the name.
func ResolveKind(names []string, q string) (string, error) {
	if q == "" {
		return "", nil
	}
	var exact, suffix []string
	seen := map[string]bool{}
	for _, n := range names {
		if n == "" || seen[n] {
			continue
		}
		seen[n] = true
		if n == q {
			exact = append(exact, n)
		}
		if strings.HasSuffix(n, q) {
			suffix = append(suffix, n)
		}
	}
	if len(exact) == 1 {
		return exact[0], nil
	}
	if len(suffix) == 1 {
		return suffix[0], nil
	}
	if len(suffix) > 1 {
		return "", fmt.Errorf("kind %q is ambiguous: %s", q, strings.Join(suffix, ", "))
	}
	return "", fmt.Errorf("unknown kind %q", q)
}

// ShortKind strips the Apple Health type prefix for display.
func ShortKind(k string) string {
	for _, p := range []string{
		"HKQuantityTypeIdentifier",
		"HKCategoryTypeIdentifier",
		"HKWorkoutActivityType",
		"HKCorrelationTypeIdentifier",
	} {
		if strings.HasPrefix(k, p) {
			return k[len(p):]
		}
	}
	return k
}
