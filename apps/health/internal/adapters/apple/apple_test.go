package apple

import "testing"

func TestSourceRankExplicit(t *testing.T) {
	if SourceRank("Apple Watch") != 10 || SourceRank("iPhone") != 20 || SourceRank("Pace") != 30 {
		t.Fatalf("Watch=%d iPhone=%d Pace=%d", SourceRank("Apple Watch"), SourceRank("iPhone"), SourceRank("Pace"))
	}
	if SourceRank("SomePhone") != RankUnlisted || SourceRank("SomeApp") != RankUnlisted {
		t.Fatal("unlisted names get RankUnlisted")
	}
	if SourceRank("Apple Watch") >= SourceRank("iPhone") || SourceRank("iPhone") >= SourceRank("Pace") {
		t.Fatal("rank order Watch < iPhone < Pace")
	}
	if SourceRank("Mindful") != 50 || SourceRank("Health") != 40 || SourceRank("Sleep") != 60 {
		t.Fatalf("Health=%d Mindful=%d Sleep=%d", SourceRank("Health"), SourceRank("Mindful"), SourceRank("Sleep"))
	}
	got := ConfiguredSources()
	if len(got) != 6 || got[0] != "Apple Watch" || got[1] != "iPhone" {
		t.Fatalf("ConfiguredSources %v", got)
	}
}

func TestDedupKeyIncludesMetadata(t *testing.T) {
	base := []string{"HKQuantityTypeIdentifierHeartRate", "Apple Watch", "2025-09-12 20:04:07 +0200", "2025-09-12 20:04:07 +0200", "68"}
	a := DedupKey(base[0], base[1], base[2], base[3], base[4], []Meta{{"HKMetadataKeyHeartRateMotionContext", "0"}})
	b := DedupKey(base[0], base[1], base[2], base[3], base[4], []Meta{{"HKMetadataKeyHeartRateMotionContext", "2"}})
	c := DedupKey(base[0], base[1], base[2], base[3], base[4], []Meta{{"HKMetadataKeyHeartRateMotionContext", "0"}})
	if a == b {
		t.Fatal("motion context 0 and 2 must not share a key")
	}
	if a != c {
		t.Fatal("same metadata must match")
	}
}
