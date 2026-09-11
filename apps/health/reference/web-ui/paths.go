package ui

import (
	"strings"

	"archive/config"
	"archive/internal/projection"
)

func kindSlug(kind string) string {
	return strings.ToLower(projection.ShortKind(kind))
}

func kindSection(kind string) string {
	sp := config.Lookup(kind)
	switch sp.Domain {
	case config.DomainMedicine, config.DomainLifestyle, config.DomainSports:
		return sp.Domain
	default:
		return "all"
	}
}

func kindPath(kind string) string {
	return "/" + kindSection(kind) + "/" + kindSlug(kind)
}

func workoutPath(kind, id string) string {
	if id == "" {
		return kindPath(kind)
	}
	return kindPath(kind) + "/" + id
}

func matchKindSlug(cat []projection.KindInfo, domain, slug string) (projection.KindInfo, bool) {
	slug = strings.ToLower(strings.TrimSpace(slug))
	if slug == "" {
		return projection.KindInfo{}, false
	}
	var hits []projection.KindInfo
	for _, ki := range cat {
		if kindSlug(ki.Kind) != slug {
			continue
		}
		sec := kindSection(ki.Kind)
		if domain != "" && sec != domain {
			continue
		}
		hits = append(hits, ki)
	}
	if len(hits) == 1 {
		return hits[0], true
	}
	if len(hits) == 0 {
		return projection.KindInfo{}, false
	}
	for _, ki := range hits {
		if config.IsWorkoutKind(ki.Kind) {
			return ki, true
		}
	}
	return hits[0], true
}
