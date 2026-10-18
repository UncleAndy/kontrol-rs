#!/usr/bin/env bash
# Publishes a release with nothing but curl, for a machine that has no gh.
# It needs a token with the `repo` scope in GH_TOKEN (or GITHUB_TOKEN) - nothing
# else - and the tag must already be pushed, because a release points at one.
#
#   GH_TOKEN=... ./packaging/publish-curl.sh 0.1.0
#
# The release is created as a draft and published only once every file has
# uploaded, so a half-finished run never leaves a half-empty release behind.
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION="${1:?usage: GH_TOKEN=... ./packaging/publish-curl.sh <version>}"
TOKEN="${GH_TOKEN:-${GITHUB_TOKEN:-}}"
if [ -z "${TOKEN}" ]; then
    echo "Set GH_TOKEN to a token with the repo scope." >&2
    exit 1
fi

REPO="${REPO:-UncleAndy/kontrol-rs}"
TAG="v${VERSION}"
API="https://api.github.com/repos/${REPO}"
AUTH=(-H "Authorization: Bearer ${TOKEN}"
      -H "Accept: application/vnd.github+json"
      -H "X-GitHub-Api-Version: 2022-11-28")

echo "Creating draft release ${TAG} in ${REPO}"
RELEASE_JSON="$(curl -sS -X POST "${AUTH[@]}" "${API}/releases" \
    -d "{\"tag_name\":\"${TAG}\",\"name\":\"komplement ${VERSION}\",\"draft\":true}")"
# The first "id" in the reply is the release's own.
RELEASE_ID="$(printf '%s' "${RELEASE_JSON}" | grep -o '"id": *[0-9]*' | head -1 | grep -o '[0-9]*')"
if [ -z "${RELEASE_ID}" ]; then
    echo "GitHub said: ${RELEASE_JSON}" >&2
    exit 1
fi

for asset in dist/*.deb dist/*.rpm dist/*.run; do
    [ -e "${asset}" ] || continue
    echo "Uploading ${asset}"
    curl -sS -X POST "${AUTH[@]}" \
        -H "Content-Type: application/octet-stream" \
        --data-binary "@${asset}" \
        "${API}/releases/${RELEASE_ID}/assets?name=$(basename "${asset}")" >/dev/null
done

echo "Publishing ${TAG}"
curl -sS -X PATCH "${AUTH[@]}" "${API}/releases/${RELEASE_ID}" \
    -d '{"draft":false}' >/dev/null

echo "Done: https://github.com/${REPO}/releases/tag/${TAG}"
