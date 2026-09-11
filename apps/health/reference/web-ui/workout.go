package ui

import (
	"encoding/json"
	"strconv"
	"strings"

	"archive/config"
	"archive/internal/projection"
)

type workoutView struct {
	ID       string
	Kind     string
	Display  string
	Source   string
	Start    string
	End      string
	Day      string
	Duration string
	Distance string
	Energy   string
	HRAvg    string
	HRMax    string
	HasRoute bool
	Href     string
}

func parseWorkout(ep projection.EpisodeRow, dayFn func(string) string) workoutView {
	w := workoutView{
		ID:      ep.DedupKey,
		Kind:    ep.Kind,
		Display: config.DisplayOf(ep.Kind),
		Source:  ep.Source,
		Start:   ep.Start,
		End:     ep.End,
		Href:    workoutPath(ep.Kind, ep.DedupKey),
	}
	var body map[string]any
	_ = json.Unmarshal([]byte(ep.Body), &body)
	if body == nil {
		body = map[string]any{}
	}
	w.Duration = joinNumUnit(asString(body["duration"]), asString(body["durationUnit"]))
	if d := asString(body["totalDistance"]); d != "" {
		w.Distance = joinNumUnit(d, asString(body["totalDistanceUnit"]))
	}
	if e := asString(body["totalEnergyBurned"]); e != "" {
		w.Energy = joinNumUnit(e, asString(body["totalEnergyBurnedUnit"]))
	}
	if stats, ok := body["statistics"].([]any); ok {
		for _, raw := range stats {
			st, ok := raw.(map[string]any)
			if !ok {
				continue
			}
			typ := asString(st["type"])
			unit := asString(st["unit"])
			switch typ {
			case "HKQuantityTypeIdentifierDistanceWalkingRunning", "HKQuantityTypeIdentifierDistanceCycling":
				if w.Distance == "" {
					w.Distance = joinNumUnit(firstStat(st, "sum", "average"), unit)
				}
			case "HKQuantityTypeIdentifierActiveEnergyBurned":
				if w.Energy == "" {
					w.Energy = joinNumUnit(firstStat(st, "sum"), unit)
				}
			case "HKQuantityTypeIdentifierHeartRate":
				w.HRAvg = joinNumUnit(firstStat(st, "average"), unit)
				w.HRMax = joinNumUnit(firstStat(st, "maximum"), unit)
			}
		}
	}
	if routes, ok := body["routes"].([]any); ok && len(routes) > 0 {
		w.HasRoute = true
	}
	if dayFn != nil {
		w.Day = dayFn(ep.Start)
	}
	return w
}

func asString(v any) string {
	switch t := v.(type) {
	case string:
		return t
	case float64:
		return strconv.FormatFloat(t, 'f', -1, 64)
	default:
		return ""
	}
}

func firstStat(st map[string]any, keys ...string) string {
	for _, k := range keys {
		if s := asString(st[k]); s != "" {
			return s
		}
	}
	return ""
}

func joinNumUnit(n, unit string) string {
	n = strings.TrimSpace(n)
	if n == "" {
		return ""
	}
	if v, err := strconv.ParseFloat(n, 64); err == nil {
		n = trimFloat(v)
	}
	if unit != "" {
		return n + " " + unit
	}
	return n
}
