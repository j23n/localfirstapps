# Dropping this in

Phase 0.1 is done. This repository is the monorepo. The four apps live at
`apps/{gallery,contacts,music,health}`. The Pages site is `docs/`; the
specification is `docs/spec/`.

```
docs/spec/README.md
docs/spec/DROP-IN.md            ← this file
docs/spec/adr/0001…0008.md
docs/spec/spikes/
docs/IMPLEMENTATION-PLAN.md
docker/
apps/gallery/
apps/contacts/
apps/music/
apps/health/
```

The standalone GitHub remotes (`localgallery`, `localcontacts`,
`localmusic`, `localhealth`) still need redirect READMEs. That is not
done in this tree.

## What to read, in order

1. **`README.md`** — the product in one sentence, the eight documents, and
   *The five things most likely to be got wrong*. Fifteen minutes.
2. **`IMPLEMENTATION-PLAN.md` §4** — the four milestones and the migration
   register. This is the map.
3. **The ADRs**, but not yet. They are best read at Milestone A, against a
   tree that no longer contains the code they retire. That is the whole
   reason Phase 1 is cleanup.

## What to run first

The 0.6 spikes are answered in `docs/spec/spikes/`. Recorded decisions:
SFace + YuNet; no Flatpak and no portal; ISA drift assumed negligible.

**Next is Phase 1 — deletions** (Milestone A). The graph check is red
(`gallery-geo` → `ureq`); that is the honest state until Phase 2. Do
not start Phase 2 first.

## The one thing not to do

Do not start Phase 2. `localcore` is the interesting work and it will be
tempting, but extracting it before Phase 1's deletions means extracting from
a surface that still contains `ProviderAttrs` in the public `Vfs` trait — 33
references across the scanner and the FFI that Phase 1 removes for free.
Cleanup first is not tidiness; it is what makes the extraction a lift rather
than a rewrite.

## Honest status of this pack

- **Verified against the code:** every line count and file reference in the
  plan; `gallery-geo`'s Nominatim client and its four dependents; `ort`'s
  `download-binaries` pulling `ureq` into the graph; the absence of any
  `.sync-conflict` handling anywhere; `git subtree` losing `git log <path>`
  where `filter-repo` keeps it; the GTK shell's 27 comparator and 42
  formatting sites; `ContentVersion`'s optionality on both sides of the FFI,
  which is what makes M4 a non-event.
- **Not verified, and flagged where it appears:** the replacements for
  `PhotoExporter` and `EXIFService` are not named; the sizing in §8 is an
  estimate resting on ADR 0004 surviving Phase 3, which risk 4 says it will
  not. The three spikes are answered in `docs/spec/spikes/`.
- **Never run:** the agent container image has not been built in this
  environment, and Fedora package names in its Dockerfile could not be
  verified because the egress policy blocks the Fedora mirrors. The
  build-time assertion layer is the mitigation: it fails the build naming
  whichever binary is missing. `rustup` and `graphene-devel` are the two
  least confident names.
