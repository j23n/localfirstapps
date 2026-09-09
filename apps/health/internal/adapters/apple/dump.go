package apple

import (
	"bytes"
	"encoding/json"
	"io"
	"sort"
)

// Collect streams a small export into slices. For fixtures only — not the projection path.
func Collect(r io.Reader) (Header, []Observation, []Episode, error) {
	var obs []Observation
	var eps []Episode
	hdr, err := Stream(r, Handler{
		Observation: func(o Observation) error {
			obs = append(obs, o)
			return nil
		},
		Episode: func(e Episode) error {
			eps = append(eps, e)
			return nil
		},
	})
	return hdr, obs, eps, err
}

// WriteGolden writes canonical NDJSON of parsed output (sorted by dedup_key).
func WriteGolden(w io.Writer, obs []Observation, eps []Episode) error {
	sort.Slice(obs, func(i, j int) bool { return obs[i].DedupKey < obs[j].DedupKey })
	sort.Slice(eps, func(i, j int) bool { return eps[i].DedupKey < eps[j].DedupKey })
	enc := json.NewEncoder(w)
	enc.SetEscapeHTML(false)
	for _, o := range obs {
		if err := enc.Encode(struct {
			Class string      `json:"class"`
			Item  Observation `json:"item"`
		}{"observation", o}); err != nil {
			return err
		}
	}
	for _, e := range eps {
		if err := enc.Encode(struct {
			Class string  `json:"class"`
			Item  Episode `json:"item"`
		}{"episode", e}); err != nil {
			return err
		}
	}
	return nil
}

// GoldenBytes is the canonical golden payload for a fixture reader.
func GoldenBytes(r io.Reader) ([]byte, error) {
	_, obs, eps, err := Collect(r)
	if err != nil {
		return nil, err
	}
	var buf bytes.Buffer
	if err := WriteGolden(&buf, obs, eps); err != nil {
		return nil, err
	}
	return buf.Bytes(), nil
}
