#!/bin/sh
# Print the CHANGELOG.md body for a version (tag v1.2.3 or bare 1.2.3).
# Exits 1 if that heading is missing.
set -eu

root=$(git rev-parse --show-toplevel)
ver=${1:?usage: scripts/changelog-notes.sh v1.2.3}
ver=${ver#v}
file=${CHANGELOG_FILE:-$root/CHANGELOG.md}

if [ ! -f "$file" ]; then
	echo "missing $file" >&2
	exit 1
fi

awk -v ver="$ver" '
	BEGIN { heading = "^## \\[(v)?" ver "\\]" }
	$0 ~ heading { grab = 1; next }
	grab && /^## / { exit }
	grab { body = body $0 "\n" }
	END {
		if (!grab) {
			print "CHANGELOG.md has no ## [" ver "] section" > "/dev/stderr"
			exit 1
		}
		sub(/\n+$/, "", body)
		if (body == "") {
			print "CHANGELOG.md section ## [" ver "] is empty" > "/dev/stderr"
			exit 1
		}
		print body
	}
' "$file"
