#!/usr/bin/env bash
# Container entrypoint. Runs as `agent`, never as root.
#
# Everything here is idempotent and cheap: it runs on every `compose up` and
# on nothing else. Its job is to make the persistent volumes usable and to
# fail loudly, by name, when one of them is not — a container whose ~/.ssh is
# unwritable otherwise presents as "git push asks for a password forever".
set -euo pipefail

warn()  { printf '\033[33mwarn:\033[0m  %s\n' "$*" >&2; }
fatal() { printf '\033[31mfatal:\033[0m %s\n' "$*" >&2; exit 1; }

# ---------------------------------------------------------------------------
# Volume health.
#
# A named volume mounted over a directory that exists in the image inherits
# that directory's ownership on first use, which is why the Dockerfile creates
# them as `agent` before declaring them. A volume created by an older
# compose file, or a host path bind-mounted here instead, will not have been —
# and the agent user cannot chown its way out. Say so precisely.
# ---------------------------------------------------------------------------
for d in "$HOME/.ssh" "$HOME/.config" "$HOME/.claude" "$HOME/.cursor" "$HOME/.cache"; do
  [ -d "$d" ] || mkdir -p "$d"
  if ! [ -w "$d" ]; then
    fatal "$d is not writable by $(id -un) (uid $(id -u)).
       The volume was created with the wrong ownership. Fix from the host:
         docker compose down
         docker volume rm localfiles_$(basename "$d" | tr -d '.')
         docker compose up -d
       Or, to keep its contents:
         docker run --rm -v localfiles_$(basename "$d" | tr -d '.'):/v alpine chown -R $(id -u):$(id -g) /v"
  fi
done

# ssh refuses to use a directory group- or world-readable, and refuses a
# private key that is not 0600. Both are ours to fix, so fix them rather than
# reporting them.
chmod 700 "$HOME/.ssh" 2>/dev/null \
  || warn "could not chmod ~/.ssh to 0700 — ssh will refuse to use it. The
       volume is writable but owned by another user."
find "$HOME/.ssh" -maxdepth 1 -type f \
     ! -name 'known_hosts*' ! -name 'config' ! -name '*.pub' \
     -exec chmod 600 {} + 2>/dev/null || true

# ---------------------------------------------------------------------------

# Workspace-level agent instructions. Claude Code reads ~/.claude/CLAUDE.md as
# user memory, the one file guaranteed to be in scope whichever repo under
# /work an agent is started in. Seeded once; yours thereafter.
if [ -r /usr/local/share/agent/CLAUDE.md ] && [ ! -e "$HOME/.claude/CLAUDE.md" ]; then
  cp /usr/local/share/agent/CLAUDE.md "$HOME/.claude/CLAUDE.md"
fi

# ---------------------------------------------------------------------------
# rustup shims.
#
# Laid down here rather than at build time because CARGO_HOME is a volume: a
# build-time install would be captured into the volume on first use and then
# shadow every later image. `rustup-init` is a no-op once the shims exist.
#
# --default-toolchain none is the point: no compiler is installed, so the
# checkout's rust-toolchain.toml decides which one rustup fetches on first
# cargo invocation.
# ---------------------------------------------------------------------------
if command -v rustup-init >/dev/null && ! [ -x "${CARGO_HOME:-$HOME/.cargo}/bin/rustup" ]; then
  rustup-init -y --no-modify-path --default-toolchain none --profile minimal >/dev/null
fi

# ---------------------------------------------------------------------------
# Advisory checks. None of these are fatal — an agent doing read-only work
# needs none of them — but each one is a thing you would rather learn now than
# at the moment a push fails.
# ---------------------------------------------------------------------------
if [ -z "$(git config --get user.email || true)" ] && [ -z "${GIT_AUTHOR_EMAIL:-}" ]; then
  warn "no git identity. Set it once — it persists in the config volume:
         git config --global user.name  'Your Name'
         git config --global user.email 'you@example.com'"
fi

if command -v gh >/dev/null && ! gh auth status >/dev/null 2>&1; then
  warn "gh is not authenticated. Run 'gh auth login' once; it persists in the
       config volume. Choose SSH as the git protocol if you added a key above."
fi

if [ -z "$(ls -A "$HOME/.ssh" 2>/dev/null | grep -v '^known_hosts' | grep -v '^config$' || true)" ]; then
  warn "no ssh keys in the .ssh volume. Generate one here and add the public
       half to GitHub — it persists across restarts:
         ssh-keygen -t ed25519 -C '<you>' && gh ssh-key add ~/.ssh/id_ed25519.pub"
fi

exec "$@"
