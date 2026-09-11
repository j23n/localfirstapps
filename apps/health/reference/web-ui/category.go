package ui

import (
	"html/template"
	"strings"

	"archive/config"
)

// Apple Health category slugs. Colour is identity, confined to the icon
// and the 14px favourite label.
const (
	catActivity = "activity" // energy / steps / exercise
	catHeart    = "heart"
	catSleep    = "sleep"
	catWalk     = "walk" // nutrition / walking / distance
	catMind     = "mind"
	catBody     = "body"
)

func categoryOf(kind string) string {
	k := strings.ToLower(kind)
	switch {
	case strings.Contains(k, "sleep"):
		return catSleep
	case strings.Contains(k, "heart"), strings.Contains(k, "hrv"),
		strings.Contains(k, "vo2"), strings.Contains(k, "respiratory"),
		strings.Contains(k, "oxygen"):
		return catHeart
	case strings.Contains(k, "workout"):
		return catMind
	case strings.Contains(k, "mindful"), strings.Contains(k, "headache"),
		strings.Contains(k, "audio"):
		return catMind
	case strings.Contains(k, "bodymass"), strings.Contains(k, "height"),
		strings.Contains(k, "bodyfat"), strings.Contains(k, "bmi"),
		strings.Contains(k, "waist"):
		return catBody
	case strings.Contains(k, "dietary"), strings.Contains(k, "caffeine"),
		strings.Contains(k, "nutrition"), strings.Contains(k, "water"):
		return catWalk
	case strings.Contains(k, "distance"), strings.Contains(k, "walk"),
		strings.Contains(k, "flights"), strings.Contains(k, "cycling"),
		strings.Contains(k, "stair"):
		return catWalk
	case strings.Contains(k, "step"), strings.Contains(k, "energy"),
		strings.Contains(k, "exercise"), strings.Contains(k, "stand"),
		strings.Contains(k, "activity"), strings.Contains(k, "physicaleffort"),
		strings.Contains(k, "running"), strings.Contains(k, "daylight"):
		return catActivity
	}
	switch config.DomainOf(kind) {
	case config.DomainMedicine:
		return catHeart
	case config.DomainLifestyle:
		return catBody
	case config.DomainSports:
		return catActivity
	default:
		return catBody
	}
}

func baselineSide(note string) string {
	n := strings.ToLower(note)
	if strings.HasPrefix(n, "lower") {
		return "below"
	}
	if strings.HasPrefix(n, "higher") {
		return "above"
	}
	return ""
}

func categoryIcon(kind string) template.HTML {
	cat := categoryOf(kind)
	path := iconPath[cat]
	if path == "" {
		path = iconPath[catBody]
	}
	return template.HTML(`<svg class="ico-svg" viewBox="0 0 24 24" width="18" height="18" aria-hidden="true"><path fill="currentColor" d="` + path + `"/></svg>`)
}

// Hand-drawn 24px marks — not a font, not a framework.
var iconPath = map[string]string{
	catActivity: "M12 2.4c.3 3.4 1.4 5.4 3.8 7.2-1.7.1-2.8.7-3.6 2.1 2.1.1 3.5 1.2 4.4 2.9-1.6 3.2-5.2 4.2-8.2 1.6C6.2 14.4 6 11 8.2 8.2 9.6 6.3 10.8 4.4 12 2.4z",
	catHeart:    "M12 20.4S4.6 15.6 2.8 12.1C1.3 9.2 2.6 5.6 6.2 5.6c1.9 0 3.2 1.2 3.8 2.3.6-1.1 1.9-2.3 3.8-2.3 3.6 0 4.9 3.6 3.4 6.5-1.8 3.5-9.2 8.3-9.2 8.3z",
	catSleep:    "M15.2 3.2a8.4 8.4 0 1 0 5.6 14.2 7.2 7.2 0 0 1-5.6-14.2z",
	catWalk:     "M13.2 4.6a1.7 1.7 0 1 1-3.4 0 1.7 1.7 0 0 1 3.4 0zM8.2 8.4l2.6-.2 1.3 3.1 1.8-1.6 2.6 1.2-.8 1.6-1.7-.8-1.6 1.5 2.1 5.1-1.8.6-1.8-4.3-1.4 2.2-2.9 1.1-.6-1.6 3.1-1.2-.9-2.2-2.2.2.2-1.7z",
	catMind:     "M12 4.2c2.6 0 4.2 1.7 4.8 3.3 1.6.4 2.8 1.8 2.8 3.6 0 1.4-.8 2.6-1.9 3.2.2 2.2-1.2 4.3-3.6 5.1v1.4h-4.2v-1.4c-2.4-.8-3.8-2.9-3.6-5.1-1.1-.6-1.9-1.8-1.9-3.2 0-1.8 1.2-3.2 2.8-3.6C7.8 5.9 9.4 4.2 12 4.2z",
	catBody:     "M12 4.4a1.8 1.8 0 1 1 0 3.6 1.8 1.8 0 0 1 0-3.6zM8.4 9.2h7.2c.6 0 1 .4 1 1v4.4h-1.8v5.2H13V14h-2v5.8H9.2v-5.2H7.4V10.2c0-.6.4-1 1-1z",
}
