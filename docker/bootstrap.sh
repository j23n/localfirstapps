#!/usr/bin/env bash
# One-time host setup. Idempotent — re-run it after moving the workspace.
#
# Much smaller than it was: there is no agent-home to create, no ssh socket to
# locate, and no token to place. Credentials live in Docker volumes now and are
# written from inside the container.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"   # localapps/docker
workspace="$(cd "$here/../.." && pwd)"                  # ~/localfiles
env_file="$here/.env"

echo "workspace: $workspace"

missing=()
for r in localapps localgallery localcontacts localmusic localhealth; do
  [ -d "$workspace/$r/.git" ] || missing+=("$r")
done
if [ ${#missing[@]} -gt 0 ]; then
  echo "note: not a git checkout under $workspace: ${missing[*]}" >&2
  echo "      clone them there, or set WORKSPACE in .env to where they are." >&2
fi

if [ -f "$env_file" ]; then
  echo "keeping existing $env_file"
else
  cat > "$env_file" <<EOF
WORKSPACE=$workspace
UID=$(id -u)
GID=$(id -g)
GIT_AUTHOR_NAME=$(git config --global user.name 2>/dev/null || true)
GIT_AUTHOR_EMAIL=$(git config --global user.email 2>/dev/null || true)
TZ=$(readlink /etc/localtime 2>/dev/null | sed 's#.*/zoneinfo/##' || echo UTC)
AGENT_CPUS=4
AGENT_MEM=8g
INSTALL_CURSOR=1
EXTRA_PACKAGES=
EOF
  chmod 600 "$env_file"
  echo "wrote $env_file"
fi

cat <<'EOF'

next:
  docker compose build
  docker compose up -d
  docker compose exec agent bash

then, once, inside the container — all of it persists in volumes:
  ssh-keygen -t ed25519 -C '<you>'
  gh auth login                          # choose SSH as the git protocol
  gh ssh-key add ~/.ssh/id_ed25519.pub
  claude                                 # /login on first run
EOF
