package apple

import (
	"sort"

	"archive/config"
)

// RankUnlisted is assigned to any sourceName not in the configured table.
const RankUnlisted = 100

// SourceRank returns the configured rank for name.
func SourceRank(name string) int {
	return config.SourceRank(name)
}

// ConfiguredSources returns source names in rank order (then name).
func ConfiguredSources() []string {
	ranks := config.SourceRanks()
	out := make([]string, 0, len(ranks))
	for n := range ranks {
		out = append(out, n)
	}
	sort.Slice(out, func(i, j int) bool {
		if ranks[out[i]] != ranks[out[j]] {
			return ranks[out[i]] < ranks[out[j]]
		}
		return out[i] < out[j]
	})
	return out
}
