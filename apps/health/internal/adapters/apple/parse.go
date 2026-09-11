package apple

import (
	"encoding/xml"
	"fmt"
	"io"
	"sort"
	"strings"
)

// Handler receives streamed items. The parser does not accumulate Records.
type Handler struct {
	Observation func(Observation) error
	Episode     func(Episode) error
}

// Stream tokenizes export.xml. It never validates the DTD and never loads HealthData.
func Stream(r io.Reader, h Handler) (Header, error) {
	dec := xml.NewDecoder(r)
	dec.Strict = false
	var hdr Header
	var exportOffset string
	for {
		tok, err := dec.Token()
		if err == io.EOF {
			return hdr, nil
		}
		if err != nil {
			return hdr, err
		}
		se, ok := tok.(xml.StartElement)
		if !ok {
			continue
		}
		switch se.Name.Local {
		case "HealthData":
			hdr.Locale = attr(se, "locale")
		case "ExportDate":
			hdr.ExportDate = attr(se, "value")
			if _, off, err := ParseTime(hdr.ExportDate); err == nil {
				exportOffset = off
			}
		case "Me":
			hdr.Me = attrsMap(se)
		case "Record":
			obs, err := readRecord(dec, se)
			if err != nil {
				return hdr, err
			}
			if h.Observation != nil {
				if err := h.Observation(obs); err != nil {
					return hdr, err
				}
			}
		case "Correlation":
			ep, err := readCorrelation(dec, se)
			if err != nil {
				return hdr, err
			}
			if h.Episode != nil {
				if err := h.Episode(ep); err != nil {
					return hdr, err
				}
			}
		case "Workout":
			ep, err := readWorkout(dec, se)
			if err != nil {
				return hdr, err
			}
			if h.Episode != nil {
				if err := h.Episode(ep); err != nil {
					return hdr, err
				}
			}
		case "ActivitySummary":
			ep, err := readActivitySummary(se, exportOffset)
			if err != nil {
				return hdr, err
			}
			if h.Episode != nil {
				if err := h.Episode(ep); err != nil {
					return hdr, err
				}
			}
		default:
			ep, err := readGeneric(dec, se)
			if err != nil {
				return hdr, err
			}
			if h.Episode != nil {
				if err := h.Episode(ep); err != nil {
					return hdr, err
				}
			}
		}
	}
}

func readRecord(dec *xml.Decoder, se xml.StartElement) (Observation, error) {
	a := attrsMap(se)
	obs := Observation{
		Kind:          a["type"],
		Source:        a["sourceName"],
		SourceVersion: a["sourceVersion"],
		Device:        a["device"],
		Unit:          a["unit"],
		Value:         a["value"],
	}
	if obs.Kind == "" {
		return obs, fmt.Errorf("record missing type")
	}
	var err error
	if a["startDate"] != "" {
		obs.Start, obs.StartOffset, err = ParseTime(a["startDate"])
		if err != nil {
			return obs, fmt.Errorf("record startDate %q: %w", a["startDate"], err)
		}
	}
	if a["endDate"] != "" {
		obs.End, obs.EndOffset, err = ParseTime(a["endDate"])
		if err != nil {
			return obs, fmt.Errorf("record endDate %q: %w", a["endDate"], err)
		}
	}
	for {
		tok, err := dec.Token()
		if err != nil {
			return obs, err
		}
		switch t := tok.(type) {
		case xml.EndElement:
			if t.Name.Local == "Record" {
				sort.Slice(obs.Metadata, func(i, j int) bool {
					if obs.Metadata[i].Key != obs.Metadata[j].Key {
						return obs.Metadata[i].Key < obs.Metadata[j].Key
					}
					return obs.Metadata[i].Value < obs.Metadata[j].Value
				})
				obs.DedupKey = DedupKey(obs.Kind, obs.Source, a["startDate"], a["endDate"], obs.Value, obs.Metadata)
				return obs, nil
			}
		case xml.StartElement:
			switch t.Name.Local {
			case "MetadataEntry":
				obs.Metadata = append(obs.Metadata, Meta{Key: attr(t, "key"), Value: attr(t, "value")})
			case "HeartRateVariabilityMetadataList":
				bpms, err := readHRV(dec)
				if err != nil {
					return obs, err
				}
				obs.HRV = bpms
			default:
				if err := skip(dec, t.Name.Local); err != nil {
					return obs, err
				}
			}
		}
	}
}

func readHRV(dec *xml.Decoder) ([]BPM, error) {
	var out []BPM
	for {
		tok, err := dec.Token()
		if err != nil {
			return nil, err
		}
		switch t := tok.(type) {
		case xml.EndElement:
			if t.Name.Local == "HeartRateVariabilityMetadataList" {
				return out, nil
			}
		case xml.StartElement:
			if t.Name.Local == "InstantaneousBeatsPerMinute" {
				out = append(out, BPM{BPM: attr(t, "bpm"), Time: attr(t, "time")})
			} else if err := skip(dec, t.Name.Local); err != nil {
				return nil, err
			}
		}
	}
}

func readCorrelation(dec *xml.Decoder, se xml.StartElement) (Episode, error) {
	a := attrsMap(se)
	var meta []Meta
	nested := 0
	for {
		tok, err := dec.Token()
		if err != nil {
			return Episode{}, err
		}
		switch t := tok.(type) {
		case xml.EndElement:
			if t.Name.Local == "Correlation" {
				return correlationEpisode(a, meta, nested)
			}
		case xml.StartElement:
			switch t.Name.Local {
			case "MetadataEntry":
				meta = append(meta, Meta{Key: attr(t, "key"), Value: attr(t, "value")})
			case "Record":
				// UNVERIFIED: this export has zero Correlation elements. The
				// DTD claims nested Records also appear top-level; skip is
				// implemented but has never run against real data.
				if _, err := readRecord(dec, t); err != nil {
					return Episode{}, err
				}
				nested++
			default:
				if err := skip(dec, t.Name.Local); err != nil {
					return Episode{}, err
				}
			}
		}
	}
}

func correlationEpisode(a map[string]string, meta []Meta, nested int) (Episode, error) {
	ep := Episode{
		Kind:   a["type"],
		Source: a["sourceName"],
		Body:   map[string]any{"nested_records_skipped": nested},
	}
	if len(meta) > 0 {
		sort.Slice(meta, func(i, j int) bool { return meta[i].Key < meta[j].Key })
		ep.Body["metadata"] = meta
	}
	copyVersions(ep.Body, a)
	if err := fillTimes(&ep, a); err != nil {
		return ep, err
	}
	ep.DedupKey = DedupKey(ep.Kind, ep.Source, a["startDate"], a["endDate"], "", nil)
	return ep, nil
}

func readWorkout(dec *xml.Decoder, se xml.StartElement) (Episode, error) {
	a := attrsMap(se)
	var (
		meta   []Meta
		events []map[string]string
		stats  []map[string]string
		routes []map[string]string
	)
	for {
		tok, err := dec.Token()
		if err != nil {
			return Episode{}, err
		}
		switch t := tok.(type) {
		case xml.EndElement:
			if t.Name.Local == "Workout" {
				return workoutEpisode(a, meta, events, stats, routes)
			}
		case xml.StartElement:
			switch t.Name.Local {
			case "MetadataEntry":
				meta = append(meta, Meta{Key: attr(t, "key"), Value: attr(t, "value")})
			case "WorkoutEvent":
				ev, err := readAttrsElement(dec, t)
				if err != nil {
					return Episode{}, err
				}
				events = append(events, ev)
			case "WorkoutStatistics":
				st, err := readAttrsElement(dec, t)
				if err != nil {
					return Episode{}, err
				}
				stats = append(stats, st)
			case "WorkoutRoute":
				rt, err := readRoute(dec, t)
				if err != nil {
					return Episode{}, err
				}
				routes = append(routes, rt)
			default:
				if err := skip(dec, t.Name.Local); err != nil {
					return Episode{}, err
				}
			}
		}
	}
}

func workoutEpisode(a map[string]string, meta []Meta, events, stats, routes []map[string]string) (Episode, error) {
	ep := Episode{
		Kind:   a["workoutActivityType"],
		Source: a["sourceName"],
		Body:   map[string]any{},
	}
	for _, k := range []string{"duration", "durationUnit", "totalDistance", "totalDistanceUnit", "totalEnergyBurned", "totalEnergyBurnedUnit", "sourceVersion", "device"} {
		if v := a[k]; v != "" {
			ep.Body[k] = v
		}
	}
	if len(meta) > 0 {
		sort.Slice(meta, func(i, j int) bool { return meta[i].Key < meta[j].Key })
		ep.Body["metadata"] = meta
	}
	if len(events) > 0 {
		ep.Body["events"] = events
	}
	if len(stats) > 0 {
		ep.Body["statistics"] = stats
	}
	if len(routes) > 0 {
		ep.Body["routes"] = routes
	}
	if err := fillTimes(&ep, a); err != nil {
		return ep, err
	}
	ep.DedupKey = DedupKey(ep.Kind, ep.Source, a["startDate"], a["endDate"], a["duration"], nil)
	return ep, nil
}

func readRoute(dec *xml.Decoder, se xml.StartElement) (map[string]string, error) {
	m := attrsMap(se)
	for {
		tok, err := dec.Token()
		if err != nil {
			return nil, err
		}
		switch t := tok.(type) {
		case xml.EndElement:
			if t.Name.Local == "WorkoutRoute" {
				return m, nil
			}
		case xml.StartElement:
			if t.Name.Local == "FileReference" {
				m["path"] = attr(t, "path")
			} else if t.Name.Local == "MetadataEntry" {
				// keep first-class path; extra metadata is not decoded further
			} else if err := skip(dec, t.Name.Local); err != nil {
				return nil, err
			}
		}
	}
}

func readAttrsElement(dec *xml.Decoder, se xml.StartElement) (map[string]string, error) {
	m := attrsMap(se)
	if err := skip(dec, se.Name.Local); err != nil {
		return nil, err
	}
	return m, nil
}

func readActivitySummary(se xml.StartElement, exportOffset string) (Episode, error) {
	a := attrsMap(se)
	day := a["dateComponents"]
	ep := Episode{
		Kind: "ActivitySummary",
		Body: map[string]any{},
	}
	for k, v := range a {
		if k != "dateComponents" && v != "" {
			ep.Body[k] = v
		}
	}
	if day != "" {
		ep.Start, ep.End, ep.StartOffset, ep.EndOffset = activitySummaryTimes(day, exportOffset)
	}
	// Key on the calendar date only. Apple revises activeEnergyBurned
	// between overlapping exports; first-seen wins via INSERT OR IGNORE.
	ep.DedupKey = DedupKey("ActivitySummary", "", day, day, "", nil)
	return ep, nil
}

func activitySummaryTimes(day, exportOffset string) (start, end, startOff, endOff string) {
	if exportOffset != "" {
		s, so, err1 := ParseTime(day + " 00:00:00 " + exportOffset)
		e, eo, err2 := ParseTime(day + " 23:59:59 " + exportOffset)
		if err1 == nil && err2 == nil {
			return s, e, so, eo
		}
	}
	return day + "T00:00:00.000000000Z", day + "T23:59:59.000000000Z", "", ""
}

func readGeneric(dec *xml.Decoder, se xml.StartElement) (Episode, error) {
	a := attrsMap(se)
	if err := skip(dec, se.Name.Local); err != nil {
		return Episode{}, err
	}
	ep := Episode{
		Kind:   se.Name.Local,
		Source: a["sourceName"],
		Body:   map[string]any{},
	}
	for k, v := range a {
		if v != "" && k != "type" && k != "sourceName" && k != "startDate" && k != "endDate" {
			ep.Body[k] = v
		}
	}
	if t := a["type"]; t != "" {
		ep.Kind = t
	}
	if err := fillTimes(&ep, a); err != nil {
		return Episode{}, err
	}
	ep.DedupKey = DedupKey(ep.Kind, ep.Source, a["startDate"], a["endDate"], a["value"], nil)
	if ep.Start == "" {
		ep.Start = "1970-01-01T00:00:00.000000000Z"
	}
	if ep.End == "" {
		ep.End = ep.Start
	}
	return ep, nil
}

func fillTimes(ep *Episode, a map[string]string) error {
	var err error
	if a["startDate"] != "" {
		ep.Start, ep.StartOffset, err = ParseTime(a["startDate"])
		if err != nil {
			return fmt.Errorf("%s startDate %q: %w", ep.Kind, a["startDate"], err)
		}
	}
	if a["endDate"] != "" {
		ep.End, ep.EndOffset, err = ParseTime(a["endDate"])
		if err != nil {
			return fmt.Errorf("%s endDate %q: %w", ep.Kind, a["endDate"], err)
		}
	}
	return nil
}

func copyVersions(body map[string]any, a map[string]string) {
	if v := a["sourceVersion"]; v != "" {
		body["sourceVersion"] = v
	}
	if v := a["device"]; v != "" {
		body["device"] = v
	}
}

func skip(dec *xml.Decoder, name string) error {
	depth := 1
	for depth > 0 {
		tok, err := dec.Token()
		if err != nil {
			return err
		}
		switch t := tok.(type) {
		case xml.StartElement:
			if t.Name.Local == name {
				depth++
			}
		case xml.EndElement:
			if t.Name.Local == name {
				depth--
			}
		}
	}
	return nil
}

func attr(se xml.StartElement, name string) string {
	for _, a := range se.Attr {
		if a.Name.Local == name {
			return a.Value
		}
	}
	return ""
}

func attrsMap(se xml.StartElement) map[string]string {
	m := make(map[string]string, len(se.Attr))
	for _, a := range se.Attr {
		m[a.Name.Local] = a.Value
	}
	return m
}

// CorrelationDTDComment is Apple's HealthKit Export Version 14 wording.
const CorrelationDTDComment = "also appear as top-level records in this document"

func HasCorrelationDTDComment(dtd []byte) bool {
	return strings.Contains(string(dtd), CorrelationDTDComment)
}
