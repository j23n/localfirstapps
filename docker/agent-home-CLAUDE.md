# localfiles workspace

You are in the container agent workspace. The localfiles monorepo is
mounted at `/work`.

```
/work/docs/spec          family specification (ADRs)
/work/docs               Pages + IMPLEMENTATION-PLAN.md
/work/apps/gallery       photos   (Rust core + SwiftUI + GTK)
/work/apps/contacts      contacts (SwiftUI)
/work/apps/music         music    (SwiftUI)
/work/apps/health        health   (Go)
/work/docker             this container
/work/.agents            agent instructions
```

## Where the rules live

- **Family spec — `/work/docs/spec/`.** Normative. RFC 2119. It describes
  the goal state of all four apps, not the current code. Where the code
  and a MUST disagree, the code is wrong.
- **Convention — `/work/docs/spec/adr/0007-product-conventions.md`.**
  Sole home. `CONVENTIONS.md` is deleted.
- **Per-app `CLAUDE.md`.** Build and test commands, structure, and the
  landmines of that app alone.
- Per-app `docs/adr/` records decisions local to one app. It must not
  contradict the family spec; if it does, say so rather than picking one.

Read the spec before proposing an architectural change, and cite the
requirement you are conforming to (`ADR 0005 R2`) in the commit message.

## Ground rules

- **Never commit to `main`.** Branch, push, open a PR.
- `/work` is the host's real checkout, not a copy. Uncommitted work there is
  the engineer's. Do not `git checkout .`, `git clean`, or reset a tree you
  did not dirty.
- Another agent may be in the same checkout. Before a long edit,
  `git worktree add /work/.worktrees/<task> -b <branch>` and work there.
- The Apple halves cannot be built here — no Xcode, no iOS SDK. `cargo test`,
  `go test`, and the GTK shell are what this container verifies. Say
  "unverified on iOS" rather than implying a Swift build passed.
