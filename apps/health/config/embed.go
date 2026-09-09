// Package config loads kinds.toml, ui.toml, projection.toml, and sources.toml
// (stdlib only). A rebuild depends only on the archive root and the binary:
// if SetRoot was called and <archive root>/config/<name> exists, that file is
// used; otherwise the embedded default is used. The process working directory
// and environment are never consulted.
package config

import (
	"embed"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"sync"
	"time"
)

//go:embed kinds.toml ui.toml projection.toml sources.toml
var embedded embed.FS

// TokenWorkout in ui.toml favourites means every HKWorkoutActivityType* session.
const TokenWorkout = "workout"

// rank assigned to a sourceName not listed in sources.toml.
const rankUnlisted = 100

// Spec is one kind's display and aggregation rules.
type Spec struct {
	Kind      string
	Domain    string // medicine | lifestyle | sports | other
	Aggregate string // sum | mean | max | min | latest | count
	Display   string
	Fallback  bool
}

const (
	DomainMedicine  = "medicine"
	DomainLifestyle = "lifestyle"
	DomainSports    = "sports"
	DomainOther     = "other"
)

var (
	mu              sync.RWMutex
	kinds           map[string]Spec
	favs            []string
	pauseThresholdS int
	sourceRanks     map[string]int
	loaded          bool
)

// SetRoot records the archive root. Config overrides are read from
// <root>/config/<name>.toml; anything missing falls back to the embedded
// defaults. Call once at startup, before any lookup.
func SetRoot(root string) {
	mu.Lock()
	defer mu.Unlock()
	archiveRoot = root
	loaded = false
}

var archiveRoot string

func ensure() {
	mu.Lock()
	defer mu.Unlock()
	if loaded {
		return
	}
	kinds = map[string]Spec{}
	raw, err := readTOML("kinds.toml")
	if err == nil {
		parseKinds(raw)
	}
	ui, err := readTOML("ui.toml")
	if err == nil {
		favs = parseFavourites(ui)
	}
	pauseThresholdS = 60
	if proj, err := readTOML("projection.toml"); err == nil {
		pauseThresholdS = parsePauseThreshold(proj, pauseThresholdS)
	}
	sourceRanks = map[string]int{}
	if src, err := readTOML("sources.toml"); err == nil {
		sourceRanks = parseSources(src)
	}
	loaded = true
}

func readTOML(name string) (string, error) {
	if archiveRoot != "" {
		p := filepath.Join(archiveRoot, "config", name)
		if b, err := os.ReadFile(p); err == nil {
			return string(b), nil
		}
	}
	b, err := embedded.ReadFile(name)
	return string(b), err
}

// Lookup returns the configured spec, or the safe fallback.
func Lookup(kind string) Spec {
	ensure()
	mu.RLock()
	defer mu.RUnlock()
	if s, ok := kinds[kind]; ok {
		return s
	}
	return fallback(kind)
}

func fallback(kind string) Spec {
	return Spec{
		Kind:      kind,
		Domain:    DomainOther,
		Aggregate: "mean",
		Display:   shortName(kind),
		Fallback:  true,
	}
}

// Favourites is the configured favourite kind list (tokens like "workout" omitted).
func Favourites() []string {
	ensure()
	mu.RLock()
	defer mu.RUnlock()
	var out []string
	for _, k := range favs {
		if k == TokenWorkout {
			continue
		}
		out = append(out, k)
	}
	return out
}

// FavouriteWorkouts reports whether the blanket workout token is in favourites.
func FavouriteWorkouts() bool {
	ensure()
	mu.RLock()
	defer mu.RUnlock()
	for _, k := range favs {
		if k == TokenWorkout {
			return true
		}
	}
	return false
}

// IsWorkoutKind reports whether kind is an Apple workout activity type.
func IsWorkoutKind(kind string) bool {
	return strings.HasPrefix(kind, "HKWorkoutActivityType")
}

// ResetForTest clears cached config (tests only).
func ResetForTest() {
	mu.Lock()
	kinds = nil
	favs = nil
	pauseThresholdS = 0
	sourceRanks = nil
	archiveRoot = ""
	loaded = false
	mu.Unlock()
}

// PauseThreshold is how long a pause must last before it splits a route.
func PauseThreshold() time.Duration {
	ensure()
	mu.RLock()
	defer mu.RUnlock()
	if pauseThresholdS <= 0 {
		return 60 * time.Second
	}
	return time.Duration(pauseThresholdS) * time.Second
}

// SourceRanks returns a copy of the configured sourceName → rank map.
func SourceRanks() map[string]int {
	ensure()
	mu.RLock()
	defer mu.RUnlock()
	out := make(map[string]int, len(sourceRanks))
	for k, v := range sourceRanks {
		out[k] = v
	}
	return out
}

// SourceRank returns the configured rank for name, or 100 if unlisted.
func SourceRank(name string) int {
	ensure()
	mu.RLock()
	defer mu.RUnlock()
	if r, ok := sourceRanks[name]; ok {
		return r
	}
	return rankUnlisted
}

func parsePauseThreshold(s string, fallback int) int {
	for _, line := range tomlLines(s) {
		k, v, ok := splitKV(line)
		if !ok || k != "pause_threshold_s" {
			continue
		}
		n, err := strconv.Atoi(v)
		if err == nil && n > 0 {
			return n
		}
	}
	return fallback
}

func parseKinds(s string) {
	var cur string
	for _, line := range tomlLines(s) {
		if strings.HasPrefix(line, "[") && strings.HasSuffix(line, "]") {
			cur = strings.TrimSpace(line[1 : len(line)-1])
			if cur == "" {
				continue
			}
			kinds[cur] = Spec{Kind: cur, Domain: DomainOther, Aggregate: "mean", Display: shortName(cur)}
			continue
		}
		if cur == "" {
			continue
		}
		k, v, ok := splitKV(line)
		if !ok {
			continue
		}
		sp := kinds[cur]
		switch k {
		case "domain":
			sp.Domain = normalizeDomain(v)
		case "daily_aggregate":
			sp.Aggregate = normalizeAgg(v)
		case "display":
			if v != "" {
				sp.Display = v
			}
		}
		kinds[cur] = sp
	}
}

func parseFavourites(s string) []string {
	var raw strings.Builder
	in := false
	for _, line := range tomlLines(s) {
		if !in {
			if !strings.HasPrefix(line, "favourites") {
				continue
			}
			in = true
			i := strings.Index(line, "[")
			if i < 0 {
				continue
			}
			line = line[i:]
		}
		raw.WriteString(line)
		raw.WriteByte(' ')
		if strings.Contains(line, "]") {
			break
		}
	}
	return splitQuotedList(raw.String())
}

func parseSources(s string) map[string]int {
	out := map[string]int{}
	in := false
	for _, line := range tomlLines(s) {
		if strings.HasPrefix(line, "[") && strings.HasSuffix(line, "]") {
			in = strings.TrimSpace(line[1:len(line)-1]) == "sources"
			continue
		}
		if !in {
			continue
		}
		k, v, ok := splitKV(line)
		if !ok {
			continue
		}
		n, err := strconv.Atoi(v)
		if err != nil {
			continue
		}
		out[k] = n
	}
	return out
}

func tomlLines(s string) []string {
	var out []string
	for _, line := range strings.Split(s, "\n") {
		line = strings.TrimSpace(stripInlineComment(line))
		if line == "" {
			continue
		}
		out = append(out, line)
	}
	return out
}

// stripInlineComment removes a # comment that is outside double-quoted strings.
func stripInlineComment(line string) string {
	inQuote := false
	for i := 0; i < len(line); i++ {
		switch line[i] {
		case '"':
			inQuote = !inQuote
		case '#':
			if !inQuote {
				return strings.TrimRight(line[:i], " \t")
			}
		}
	}
	return line
}

func splitQuotedList(s string) []string {
	s = strings.TrimSpace(s)
	if i := strings.Index(s, "["); i >= 0 {
		s = s[i+1:]
	}
	if i := strings.LastIndex(s, "]"); i >= 0 {
		s = s[:i]
	}
	var out []string
	for _, part := range strings.Split(s, ",") {
		part = strings.TrimSpace(part)
		part = strings.Trim(part, `"`)
		if part != "" {
			out = append(out, part)
		}
	}
	return out
}

func splitKV(line string) (string, string, bool) {
	i := strings.Index(line, "=")
	if i < 0 {
		return "", "", false
	}
	k := strings.Trim(strings.TrimSpace(line[:i]), `"`)
	v := strings.Trim(strings.TrimSpace(line[i+1:]), `"`)
	return k, v, k != ""
}

func normalizeDomain(s string) string {
	switch s {
	case DomainMedicine, DomainLifestyle, DomainSports, DomainOther:
		return s
	default:
		return DomainOther
	}
}

func normalizeAgg(s string) string {
	switch s {
	case "sum", "mean", "max", "min", "latest", "count":
		return s
	default:
		return "mean"
	}
}

func shortName(k string) string {
	for _, p := range []string{
		"HKQuantityTypeIdentifier",
		"HKCategoryTypeIdentifier",
		"HKWorkoutActivityType",
		"HKCorrelationTypeIdentifier",
		"HKDataType",
	} {
		if strings.HasPrefix(k, p) {
			return k[len(p):]
		}
	}
	return k
}

// DomainOf is Lookup(kind).Domain.
func DomainOf(kind string) string { return Lookup(kind).Domain }

// DisplayOf is Lookup(kind).Display.
func DisplayOf(kind string) string { return Lookup(kind).Display }
