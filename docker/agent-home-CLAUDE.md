# localfiles workspace

You are in the container agent workspace. All five repos of the localfiles
family are mounted under `/work` and you may read across all of them.

```
/work/localapps      coordination: conventions, the ADR set, this container
/work/localgallery   photos   (Rust core + SwiftUI + GTK)
/work/localcontacts  contacts (SwiftUI)
/work/localmusic     music    (SwiftUI)
/work/localhealth    health   (Go)
```

## Where the rules live

- **Family spec — `/work/localapps/docs/spec/`.** Normative. RFC 2119. It
  describes the goal state of all four apps, not the current code. Where the
  code and a MUST disagree, the code is wrong.
- **Conventions — `/work/localapps/.agents/CONVENTIONS.md`.** How things are
  done across the apps (layout, state, logging, settings, IDs, testing, CI).
  Descriptive of the house style, not of the goal state.
- **Per-repo `CLAUDE.md`.** Build and test commands, structure, and the
  landmines of that repo alone. Loaded automatically when you work in it.
- Per-repo `docs/adr/` records decisions local to one app. It must not
  contradict the family spec; if it does, say so rather than picking one.

Read the spec before proposing an architectural change, and cite the
requirement you are conforming to (`ADR 0005 R2`) in the commit message.

## Ground rules

- **Never commit to `main`.** Branch, push, open a PR.
- `/work` is the host's real checkout, not a copy. Uncommitted work there is
  the engineer's. Do not `git checkout .`, `git clean`, or reset a tree you
  did not dirty.
- Another agent may be in the same checkout. Before a long edit in a repo,
  `git worktree add /work/.worktrees/<task> -b <branch>` and work there.
- The Apple halves cannot be built here — no Xcode, no iOS SDK. `cargo test`,
  `go test`, and the GTK shell are what this container verifies. Say
  "unverified on iOS" rather than implying a Swift build passed.
- `$HOME` persists across containers; `/work` is shared. Scratch files go in
  `$HOME/scratch`, never in a repo.
