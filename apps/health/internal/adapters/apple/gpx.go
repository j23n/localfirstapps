package apple

import (
	"encoding/xml"
	"io"
	"strconv"
	"time"
)

// TrackPoint is one GPX <trkpt>. Ele is nil when <ele> is absent.
type TrackPoint struct {
	Lat  float64
	Lon  float64
	Ele  *float64
	Time time.Time
}

// maxElementText caps accumulated CharData per <ele>/<time> so a huge
// text node cannot grow the parser buffer without bound.
const maxElementText = 1024

// ParseGPX streams a GPX 1.1 document. Unknown extensions are ignored.
func ParseGPX(r io.Reader) ([]TrackPoint, error) {
	dec := xml.NewDecoder(r)
	var (
		out    []TrackPoint
		inPT   bool
		pt     TrackPoint
		hasLat bool
		hasLon bool
		eleSet bool
		buf    string
	)
	for {
		tok, err := dec.Token()
		if err == io.EOF {
			return out, nil
		}
		if err != nil {
			return nil, err
		}
		switch t := tok.(type) {
		case xml.StartElement:
			switch t.Name.Local {
			case "trkpt":
				pt = TrackPoint{}
				hasLat, hasLon, eleSet = false, false, false
				inPT = true
				for _, a := range t.Attr {
					switch a.Name.Local {
					case "lat":
						if v, err := strconv.ParseFloat(a.Value, 64); err == nil {
							pt.Lat = v
							hasLat = true
						}
					case "lon":
						if v, err := strconv.ParseFloat(a.Value, 64); err == nil {
							pt.Lon = v
							hasLon = true
						}
					}
				}
			case "ele", "time":
				buf = ""
			}
		case xml.EndElement:
			switch t.Name.Local {
			case "trkpt":
				if inPT && hasLat && hasLon {
					out = append(out, pt)
				}
				inPT = false
			case "ele":
				if inPT {
					if v, err := strconv.ParseFloat(buf, 64); err == nil {
						pt.Ele = &v
						eleSet = true
					}
					_ = eleSet
				}
			case "time":
				if inPT {
					if tm, err := parseGPXTime(buf); err == nil {
						pt.Time = tm
					}
				}
			}
		case xml.CharData:
			if inPT {
				if len(buf) >= maxElementText {
					break
				}
				chunk := string(t)
				if left := maxElementText - len(buf); len(chunk) > left {
					chunk = chunk[:left]
				}
				buf += chunk
			}
		}
	}
}

func parseGPXTime(s string) (time.Time, error) {
	s = trimSpace(s)
	for _, layout := range []string{
		time.RFC3339Nano,
		time.RFC3339,
		"2006-01-02T15:04:05Z",
	} {
		if t, err := time.Parse(layout, s); err == nil {
			return t.UTC(), nil
		}
	}
	return time.Time{}, errString("gpx time")
}

func trimSpace(s string) string {
	i, j := 0, len(s)
	for i < j && (s[i] == ' ' || s[i] == '\n' || s[i] == '\t' || s[i] == '\r') {
		i++
	}
	for j > i && (s[j-1] == ' ' || s[j-1] == '\n' || s[j-1] == '\t' || s[j-1] == '\r') {
		j--
	}
	return s[i:j]
}
