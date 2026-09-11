#!/bin/sh
# Draft or tag a release.
#
#   ./scripts/release.sh v1.2.3         # insert a git-log draft if missing; stop
#   $EDITOR CHANGELOG.md                # group into Added/Changed/Fixed, rewrite
#   ./scripts/release.sh v1.2.3 --tag   # commit CHANGELOG.md + annotated tag
#
# Does not push. CI refuses a tag whose commit has no matching heading.
set -eu

root=$(git rev-parse --show-toplevel)
cd "$root"

tag=${1:?usage: scripts/release.sh v1.2.3 [--tag]}
case $tag in
v[0-9]*.[0-9]*.[0-9]*) ;;
*)
	echo "tag must look like v1.2.3" >&2
	exit 1
	;;
esac
ver=${tag#v}
mode=${2:-draft}

has_section() {
	awk -v ver="$ver" '
		$0 ~ "^## \\[(v)?" ver "\\]" { found = 1 }
		END { exit found ? 0 : 1 }
	' CHANGELOG.md
}

if [ "$mode" != draft ] && [ "$mode" != --tag ]; then
	echo "usage: scripts/release.sh v1.2.3 [--tag]" >&2
	exit 1
fi

if [ "$mode" = draft ]; then
	if ! grep -q '^## \[Unreleased\]$' CHANGELOG.md; then
		echo "CHANGELOG.md must have an ## [Unreleased] heading" >&2
		exit 1
	fi
	if has_section; then
		echo "CHANGELOG.md already has ## [$ver]. Edit it, then: $0 $tag --tag"
		exit 0
	fi
	if ! git diff --quiet CHANGELOG.md || ! git diff --cached --quiet CHANGELOG.md; then
		echo "CHANGELOG.md has uncommitted edits; finish those before drafting $tag" >&2
		exit 1
	fi

	prev=$(git describe --tags --abbrev=0 2>/dev/null || true)
	if [ -n "$prev" ]; then
		range=$prev..HEAD
	else
		range=HEAD
	fi
	notes=$(git log --reverse --pretty=format:'- %s' "$range")
	if [ -z "$notes" ]; then
		echo "no commits in $range to put in the changelog" >&2
		exit 1
	fi

	day=$(date -u +%Y-%m-%d)
	tmp=$(mktemp)
	trap 'rm -f "$tmp"' EXIT
	awk -v ver="$ver" -v day="$day" -v notes="$notes" '
		BEGIN { block = "## [" ver "] - " day "\n\n" notes "\n" }
		/^## \[Unreleased\]$/ {
			print
			print ""
			print block
			blank = 1
			next
		}
		blank && /^$/ { blank = 0; next }
		{ print }
	' CHANGELOG.md >"$tmp"
	mv "$tmp" CHANGELOG.md
	trap - EXIT
	echo "drafted ## [$ver] from $range. Edit CHANGELOG.md, then: $0 $tag --tag"
	exit 0
fi

# --tag
other=$(git status --porcelain | awk '$NF != "CHANGELOG.md" { print }')
if [ -n "$other" ]; then
	echo "working tree has changes besides CHANGELOG.md:" >&2
	printf '%s\n' "$other" >&2
	exit 1
fi
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
	echo "tag $tag already exists" >&2
	exit 1
fi
if ! has_section; then
	echo "CHANGELOG.md has no ## [$ver] section. Run $0 $tag first." >&2
	exit 1
fi
if ! ./scripts/changelog-notes.sh "$tag" >/dev/null; then
	echo "CHANGELOG.md section ## [$ver] is missing or empty" >&2
	exit 1
fi

git add CHANGELOG.md
if git diff --cached --quiet; then
	echo "CHANGELOG.md is unchanged; nothing to commit before tagging" >&2
	exit 1
fi
git commit -m "Release $tag"
git tag -a "$tag" -m "$tag"
echo "created $tag. push with: git push origin HEAD $tag"
