package ui

import (
	"encoding/json"
	"html/template"
	"math"
	"strconv"
	"strings"
	"time"
)

type chartCfg struct {
	Type        string    `json:"type"`
	Labels      []string  `json:"labels"`
	Datasets    []chartDS `json:"datasets"`
	XTitle      string    `json:"xTitle,omitempty"`
	YTitle      string    `json:"yTitle,omitempty"`
	YFormat     string    `json:"yFormat,omitempty"`
	Color       string    `json:"color,omitempty"`
	BeginAtZero bool      `json:"beginAtZero,omitempty"`
	RangeBars   bool      `json:"rangeBars,omitempty"`
}

type chartDS struct {
	Label  string `json:"label"`
	Data   any    `json:"data"`
	Center []any  `json:"center,omitempty"`
}

func localDay(ts string, loc *time.Location) (string, error) {
	t, err := time.Parse(time.RFC3339Nano, ts)
	if err != nil {
		return "", err
	}
	return t.In(loc).Format("2006-01-02"), nil
}

func parseNum(s string) (float64, bool) {
	v, err := strconv.ParseFloat(strings.TrimSpace(s), 64)
	return v, err == nil
}

func reduce(vals []float64, mode string) float64 {
	if len(vals) == 0 {
		return 0
	}
	switch mode {
	case "sum", "count":
		var s float64
		for _, v := range vals {
			s += v
		}
		return s
	case "last":
		return vals[len(vals)-1]
	default:
		var s float64
		for _, v := range vals {
			s += v
		}
		return s / float64(len(vals))
	}
}

func trimFloat(v float64) string {
	if v == float64(int64(v)) {
		return strconv.FormatInt(int64(v), 10)
	}
	return strconv.FormatFloat(v, 'f', 2, 64)
}

// HealthKit's % unit is a fraction: value 0.28 means 28%.
// Values already above 1 are treated as percent points and left alone.
func displayAmount(v float64, unit string) (float64, string) {
	if unit == "%" && v >= 0 && v <= 1 {
		return math.Round(v*10000) / 100, "%"
	}
	return v, unit
}

func formatAmount(v float64, unit string) (value, u string) {
	v, u = displayAmount(v, unit)
	return trimFloat(v), u
}

func marshalChart(cfg chartCfg) template.JS {
	b, err := json.Marshal(cfg)
	if err != nil {
		return ""
	}
	return template.JS(b)
}
