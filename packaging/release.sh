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

Build is done. To publish, whichever of these this machine has:

  # gh out of nixpkgs, without putting it on your PATH first - the one that
  # works on NixOS as it comes
  git tag -a v${VERSION} -m "komplement ${VERSION}"
  git push origin v${VERSION}
  nix shell nixpkgs#gh --command gh release create v${VERSION} \\
      dist/*.deb dist/*.rpm dist/*.run \\
      --title "komplement ${VERSION}" --notes-file CHANGELOG.md

  # or, with gh already installed
  gh release create v${VERSION} dist/* --title "komplement ${VERSION}"

  # or with nothing but curl and a token - no gh anywhere
  GH_TOKEN=... ./packaging/publish-curl.sh ${VERSION}

  # or by hand: push the tag, open
  #   https://github.com/UncleAndyV/kontrol-rs/releases/new?tag=v${VERSION}
  # and drag dist/*.deb, *.rpm and *.run onto it

Nix users need no package at all:

  nix run github:UncleAndyV/kontrol-rs
  # or, in configuration.nix / home.nix:
  services.kontrol-rs.enable = true;

EOF
