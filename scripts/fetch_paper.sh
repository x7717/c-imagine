#!/usr/bin/env bash
# Fetch a Paper server jar for a given version family via fill.papermc.io.
# Usage: fetch_paper.sh <family, e.g. 26.3> <target-dir>
set -euo pipefail

FAMILY="${1:-26.3}"
DIR="${2:-.}"
mkdir -p "$DIR"

FAMILIES_JSON="$(curl -fsSL https://fill.papermc.io/v3/projects/paper)"
LATEST="$(python3 - "$FAMILY" <<'PY'
import json, sys
family = sys.argv[1]
data = json.load(open("/dev/stdin"))
versions = data.get("versions", {}).get(family, [])
if not versions:
    print(f"ERROR: family {family} not found", file=sys.stderr)
    sys.exit(1)
print(versions[0])
PY
)"

echo "Paper family $FAMILY -> version $LATEST"

# Resolve a downloadable build via the v3 API
URL="$(python3 - "$FAMILY" "$LATEST" <<'PY'
import json, sys, urllib.request
family, version = sys.argv[1], sys.argv[2]
req = urllib.request.Request(
    f"https://fill.papermc.io/v3/projects/paper/versions/{version}/builds/latest",
    headers={"User-Agent": "c-imagine-e2e (CI; contact: repo owner)"},
)
data = json.load(urllib.request.urlopen(req))
dl = data.get("downloads", {})
entry = dl.get("server:default") or (list(dl.values())[0] if dl else None)
if not entry or "url" not in entry:
    print("ERROR: no download url in build payload", file=sys.stderr)
    sys.exit(1)
print(entry["url"])
PY
)"

echo "downloading $URL"
curl -fSL -o "$DIR/paper-$LATEST.jar" "$URL"
ls -la "$DIR"
