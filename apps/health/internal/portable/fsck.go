package portable

import (
	"errors"
	"fmt"
	"io"
	"sort"

	"archive/internal/blobs"
	"archive/internal/event"
	"archive/internal/log"
)

// Issue is one fsck finding.
type Issue struct {
	Kind    string // mismatch, missing, orphan
	SHA256  string
	Detail  string
	EventID string
}

// Report is a read-only integrity result. Fsck never repairs.
type Report struct {
	Events int
	Blobs  int
	Issues []Issue
}

// OK is true when nothing is wrong.
func (r Report) OK() bool { return len(r.Issues) == 0 }

// Write prints a human-readable report.
func (r Report) Write(w io.Writer) {
	for _, is := range r.Issues {
		switch is.Kind {
		case "mismatch":
			fmt.Fprintf(w, "mismatch sha256=%s %s\n", is.SHA256, is.Detail)
		case "missing":
			fmt.Fprintf(w, "missing sha256=%s event=%s\n", is.SHA256, is.EventID)
		case "orphan":
			fmt.Fprintf(w, "orphan sha256=%s\n", is.SHA256)
		case "torn_tail":
			fmt.Fprintf(w, "torn tail %s\n", is.Detail)
		default:
			fmt.Fprintf(w, "%s sha256=%s %s\n", is.Kind, is.SHA256, is.Detail)
		}
	}
	if r.OK() {
		fmt.Fprintf(w, "ok %d events %d blobs\n", r.Events, r.Blobs)
		return
	}
	var nMis, nMiss, nOrph int
	for _, is := range r.Issues {
		switch is.Kind {
		case "mismatch":
			nMis++
		case "missing":
			nMiss++
		case "orphan":
			nOrph++
		}
	}
	fmt.Fprintf(w, "fsck: %d mismatch, %d missing, %d orphan (%d events, %d blobs on disk)\n",
		nMis, nMiss, nOrph, r.Events, r.Blobs)
}

// Fsck verifies blob hashes, blob_import references, and that every blob
// is referenced by some event. Read-only; never writes.
func Fsck(root string) (Report, error) {
	var rep Report
	evs, err := log.ReadAll(root)
	if err != nil {
		var torn *log.TornTailError
		if errors.As(err, &torn) {
			rep.Issues = append(rep.Issues, Issue{
				Kind:   "torn_tail",
				Detail: fmt.Sprintf("%s offset=%d", torn.Path, torn.Offset),
			})
			return rep, nil
		}
		return rep, err
	}
	rep.Events = len(evs)
	listed, err := blobs.List(root)
	if err != nil {
		return rep, err
	}
	rep.Blobs = len(listed)

	onDisk := map[string]blobs.Info{}
	for _, b := range listed {
		onDisk[b.SHA256] = b
	}

	referenced := map[string]bool{}
	for _, ev := range evs {
		hash, ok := ev.BlobSHA256()
		if !ok {
			if ev.Type == event.TypeBlobImport {
				rep.Issues = append(rep.Issues, Issue{
					Kind:    "missing",
					SHA256:  "",
					EventID: ev.ID,
					Detail:  "blob_import has no sha256",
				})
			}
			continue
		}
		referenced[hash] = true
		if ev.Type != event.TypeBlobImport {
			continue
		}
		if _, ok := onDisk[hash]; !ok {
			rep.Issues = append(rep.Issues, Issue{
				Kind:    "missing",
				SHA256:  hash,
				EventID: ev.ID,
			})
		}
	}

	var orphans []string
	for hash, info := range onDisk {
		got, _, err := blobs.HashFile(info.Path)
		if err != nil {
			rep.Issues = append(rep.Issues, Issue{
				Kind:   "mismatch",
				SHA256: hash,
				Detail: err.Error(),
			})
			continue
		}
		if got != hash {
			rep.Issues = append(rep.Issues, Issue{
				Kind:   "mismatch",
				SHA256: hash,
				Detail: fmt.Sprintf("content hashes to %s", got),
			})
		}
		if !referenced[hash] {
			orphans = append(orphans, hash)
		}
	}
	sort.Strings(orphans)
	for _, hash := range orphans {
		rep.Issues = append(rep.Issues, Issue{Kind: "orphan", SHA256: hash})
	}

	sort.Slice(rep.Issues, func(i, j int) bool {
		a, b := rep.Issues[i], rep.Issues[j]
		if a.Kind != b.Kind {
			return issueOrder(a.Kind) < issueOrder(b.Kind)
		}
		if a.SHA256 != b.SHA256 {
			return a.SHA256 < b.SHA256
		}
		return a.EventID < b.EventID
	})
	return rep, nil
}

func issueOrder(kind string) int {
	switch kind {
	case "torn_tail":
		return 0
	case "mismatch":
		return 1
	case "missing":
		return 2
	case "orphan":
		return 3
	default:
		return 9
	}
}
