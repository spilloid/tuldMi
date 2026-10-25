#!/usr/bin/env bash
# The product page names a release in its download links and text; every one must be the version in package.json.
#   scripts/site-version.sh [page]   (default: docs/index.html)
# Every three-part version number on the page counts ("v0.1.0", "tuldMi_0.1.0_x64", "tuldMi 0.1.0"); a four-part dotted
# number (an IPv4 address) is not a version and is skipped.
set -euo pipefail
cd "$(dirname "$0")/.."
page="${1:-docs/index.html}"
want="$(sed -nE 's/^[[:space:]]*"version":[[:space:]]*"([^"]+)".*/\1/p' package.json | head -n1)"
[ -n "$want" ] || { echo "no version in package.json" >&2; exit 1; }
found="$(grep -oE '(^|[^0-9.])[0-9]+[.][0-9]+[.][0-9]+([.][0-9]+)?' "$page" | sed -E 's/^[^0-9]//' | grep -vE '^[0-9]+([.][0-9]+){3}$' | sort -u || true)"
if [ -z "$found" ]; then echo "$page names no version; expected $want" >&2; exit 1; fi
if [ "$found" != "$want" ]; then echo "$page names version(s) $(echo "$found" | tr '\n' ' ')but package.json is $want" >&2; exit 1; fi
echo "$page: every version number is $want ($(grep -c -F "$want" "$page") line(s))"
