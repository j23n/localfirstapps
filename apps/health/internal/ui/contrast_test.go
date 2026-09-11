package ui

import (
	"fmt"
	"math"
	"regexp"
	"strings"
	"testing"
)

func TestDesignTokenContrast(t *testing.T) {
	css, err := assetFS.ReadFile("assets/app.css")
	if err != nil {
		t.Fatal(err)
	}
	src := string(css)
	light := cssVars(src, ":root")
	dark := cssVars(src, `html[data-theme="dark"]`)
	if light["--ink"] == "" || dark["--ink"] == "" {
		t.Fatal("missing ink tokens")
	}

	type pair struct {
		fg, bg string
		min    float64
		note   string
	}
	check := func(t *testing.T, mode string, vars map[string]string) {
		t.Helper()
		pairs := []pair{
			{"--ink", "--bg", 4.5, "ink on bg"},
			{"--ink", "--surface", 4.5, "ink on surface"},
			{"--ink-dim", "--bg", 4.5, "ink-dim on bg"},
			{"--ink-dim", "--surface", 4.5, "ink-dim on surface"},
			{"--below", "--surface", 4.5, "below on surface"},
			{"--above", "--surface", 4.5, "above on surface"},
			{"--link", "--bg", 4.5, "link on bg"},
			{"--link", "--surface", 4.5, "link on surface"},
			{"--ink-hint", "--surface", 3.0, "ink-hint on surface (non-text)"},
			{"--cat-activity", "--surface", 4.5, "activity on surface"},
			{"--cat-heart", "--surface", 4.5, "heart on surface"},
			{"--cat-sleep", "--surface", 4.5, "sleep on surface"},
			{"--cat-walk", "--surface", 4.5, "walk on surface"},
			{"--cat-mind", "--surface", 4.5, "mind on surface"},
			{"--cat-body", "--surface", 4.5, "body on surface"},
		}
		for _, p := range pairs {
			fg, bg := vars[p.fg], vars[p.bg]
			if !hexColor(fg) || !hexColor(bg) {
				t.Errorf("%s %s: not a hex (%s on %s)", mode, p.note, fg, bg)
				continue
			}
			r := contrastRatio(fg, bg)
			if r+0.01 < p.min {
				t.Errorf("%s %s: %s on %s = %.2f:1 want ≥ %.1f", mode, p.note, fg, bg, r, p.min)
			}
		}
	}
	t.Run("light", func(t *testing.T) { check(t, "light", light) })
	t.Run("dark", func(t *testing.T) { check(t, "dark", dark) })
}

func TestBaselineColoursAreNotJudgement(t *testing.T) {
	css, err := assetFS.ReadFile("assets/app.css")
	if err != nil {
		t.Fatal(err)
	}
	src := string(css)
	for _, sel := range []string{":root", `html[data-theme="dark"]`} {
		vars := cssVars(src, sel)
		for _, name := range []string{"--below", "--above"} {
			h := strings.ToLower(vars[name])
			if h == "" {
				t.Fatalf("%s %s missing", sel, name)
			}
			r, g, b := rgb(h)
			// reject saturated red or green (bad/good encoding)
			if r > 180 && g < 80 && b < 80 {
				t.Errorf("%s %s %s looks red", sel, name, h)
			}
			if g > 160 && r < 90 && b < 90 {
				t.Errorf("%s %s %s looks green", sel, name, h)
			}
		}
	}
	if strings.Contains(src, "text-transform: uppercase") {
		t.Fatal("no uppercase tracked labels")
	}
}

func TestEmbeddedFontIsWOFF2(t *testing.T) {
	b, err := assetFS.ReadFile("assets/fonts/recursive-latin.woff2")
	if err != nil {
		t.Fatal(err)
	}
	if len(b) < 1000 || string(b[:4]) != "wOF2" {
		t.Fatalf("font header %q size %d", b[:min(4, len(b))], len(b))
	}
	t.Logf("recursive-latin.woff2 %d bytes", len(b))
}

func TestCategoryOfKnownKinds(t *testing.T) {
	cases := map[string]string{
		"HKQuantityTypeIdentifierStepCount":              catActivity,
		"HKQuantityTypeIdentifierActiveEnergyBurned":     catActivity,
		"HKQuantityTypeIdentifierHeartRate":              catHeart,
		"HKQuantityTypeIdentifierRestingHeartRate":       catHeart,
		"HKCategoryTypeIdentifierSleepAnalysis":          catSleep,
		"HKQuantityTypeIdentifierDistanceWalkingRunning": catWalk,
		"HKQuantityTypeIdentifierDietaryCaffeine":        catWalk,
		"HKCategoryTypeIdentifierMindfulSession":         catMind,
		"HKWorkoutActivityTypeRunning":                   catMind,
		"HKQuantityTypeIdentifierBodyMass":               catBody,
	}
	for kind, want := range cases {
		if got := categoryOf(kind); got != want {
			t.Errorf("%s: %s want %s", kind, got, want)
		}
		if categoryIcon(kind) == "" {
			t.Errorf("%s: empty icon", kind)
		}
	}
}

func cssVars(css, selector string) map[string]string {
	out := map[string]string{}
	idx := strings.Index(css, selector+" {")
	if idx < 0 {
		idx = strings.Index(css, selector+"{")
	}
	if idx < 0 {
		return out
	}
	rest := css[idx:]
	start := strings.Index(rest, "{")
	if start < 0 {
		return out
	}
	depth := 0
	end := -1
	for i := start; i < len(rest); i++ {
		switch rest[i] {
		case '{':
			depth++
		case '}':
			depth--
			if depth == 0 {
				end = i
				i = len(rest)
			}
		}
	}
	if end < 0 {
		return out
	}
	re := regexp.MustCompile(`(--[a-z0-9-]+)\s*:\s*(#[0-9A-Fa-f]{6})`)
	for _, m := range re.FindAllStringSubmatch(rest[start:end], -1) {
		out[m[1]] = strings.ToUpper(m[2])
	}
	return out
}

func hexColor(s string) bool {
	if len(s) != 7 || s[0] != '#' {
		return false
	}
	_, err := parseHex(s[1:])
	return err == nil
}

func parseHex(s string) (int, error) {
	n := 0
	for _, c := range strings.ToUpper(s) {
		n <<= 4
		switch {
		case c >= '0' && c <= '9':
			n += int(c - '0')
		case c >= 'A' && c <= 'F':
			n += int(c - 'A' + 10)
		default:
			return 0, fmt.Errorf("bad hex")
		}
	}
	return n, nil
}

func rgb(hex string) (r, g, b float64) {
	n, _ := parseHex(hex[1:])
	return float64((n >> 16) & 0xff), float64((n >> 8) & 0xff), float64(n & 0xff)
}

func relLum(hex string) float64 {
	R, G, B := rgb(hex)
	f := func(c float64) float64 {
		c = c / 255
		if c <= 0.04045 {
			return c / 12.92
		}
		return math.Pow((c+0.055)/1.055, 2.4)
	}
	return 0.2126*f(R) + 0.7152*f(G) + 0.0722*f(B)
}

func contrastRatio(a, b string) float64 {
	l1, l2 := relLum(a), relLum(b)
	if l1 < l2 {
		l1, l2 = l2, l1
	}
	return (l1 + 0.05) / (l2 + 0.05)
}
