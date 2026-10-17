#!/usr/bin/env bash
# Builds the packages and prints the commands that publish them. It publishes
# nothing itself: the upload is a person's decision, and so is the tag.
#
#   ./packaging/release.sh 0.2.0
set -euo pipefail

cd "$(dirname "$0")/.."
VERSION="${1:?usage: ./packaging/release.sh <version>}"

./packaging/build-packages.sh "${VERSION}"

cat <<EOF

Build is done. To publish, whichever way you prefer:

  git tag -a v${VERSION} -m "komplement ${VERSION}"
  git push origin v${VERSION}
  gh release create v${VERSION} \\
      dist/*.deb dist/*.rpm dist/*.AppImage \\
      --title "komplement ${VERSION}" --notes-file CHANGELOG.md

  # or drag dist/* onto the release page yourself

Nix users need no package at all:

  nix run github:UncleAndyV/kontrol-rs
  # or, in configuration.nix / home.nix:
  services.kontrol-rs.enable = true;

EOF
