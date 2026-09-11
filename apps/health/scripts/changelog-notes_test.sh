#!/bin/sh
set -eu
root=$(git rev-parse --show-toplevel)
notes=$root/scripts/changelog-notes.sh
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT

cat >"$tmp" <<'EOF'
# Changelog

## [Unreleased]

## [1.2.3] - 2026-09-09

- first item
- second item

## [1.2.2] - 2026-01-01

- older
EOF

out=$(CHANGELOG_FILE=$tmp "$notes" v1.2.3)
echo "$out" | grep -q 'first item'
echo "$out" | grep -q 'second item'
echo "$out" | grep -qv 'older' || {
	echo "leaked previous section" >&2
	exit 1
}

if CHANGELOG_FILE=$tmp "$notes" v9.9.9 >/dev/null 2>&1; then
	echo "expected missing section to fail" >&2
	exit 1
fi

echo ok
