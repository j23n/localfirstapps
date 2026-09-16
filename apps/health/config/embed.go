// Package config loads projection.toml and sources.toml (stdlib only).
// A rebuild depends only on the archive root and the binary:
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

//go:embed projection.toml sources.toml
var embedded embed.FS

// rank assigned to a sourceName not listed in sources.toml.
const rankUnlisted = 100

var (
	mu              sync.RWMutex
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

// ResetForTest clears cached config (tests only).
func ResetForTest() {
	mu.Lock()
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

func splitKV(line string) (string, string, bool) {
	i := strings.Index(line, "=")
	if i < 0 {
		return "", "", false
	}
	k := strings.Trim(strings.TrimSpace(line[:i]), `"`)
	v := strings.Trim(strings.TrimSpace(line[i+1:]), `"`)
	return k, v, k != ""
}
