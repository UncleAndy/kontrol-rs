#!/usr/bin/env bash
# Everything the release carries, built here and nowhere else:
#
#   dist/komplement_<version>_<arch>.deb
#   dist/komplement-<version>.<arch>.rpm
#   dist/komplement-<version>-<arch>.run      (one file, unpacks and runs)
#
#   ./packaging/build-packages.sh 0.2.0
#
# Without a version it is asked of git. The binaries come out of the flake - the
# same build `nix build` makes - so the packages and the store cannot drift.
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION="${1:-$(git describe --tags --always --dirty 2>/dev/null || echo 0.0.0)}"
VERSION="${VERSION#v}"
case "$(uname -m)" in
    x86_64) ARCH=amd64 ;;
    aarch64) ARCH=arm64 ;;
    *) ARCH="$(uname -m)" ;;
esac
echo "komplement ${VERSION} for ${ARCH}"

rm -rf dist staging
mkdir -p dist staging

# The binaries, straight out of the flake.
STORE="$(nix build --no-link --print-out-paths .#default | head -1)"
cp "${STORE}/bin/komplement" "${STORE}/bin/konfigure" staging/
chmod +x staging/*

# deb and rpm from the one description.
export VERSION ARCH
nix shell nixpkgs#nfpm --command nfpm package -f packaging/nfpm.yaml -p deb -t dist/
nix shell nixpkgs#nfpm --command nfpm package -f packaging/nfpm.yaml -p rpm -t dist/

# The portable bundle: one file that is a shell header with a tar.gz payload
# appended, for a machine with no package manager and no session to speak of.
# (A real AppImage needs FUSE and the AppImageKit tools - appimagetool and
# appimagekit are gone from nixpkgs - and linuxdeploy, which is still there,
# cannot read a Nix store binary: it runs ldd, which needs a normal /usr.)
BUNDLE="${PWD}/staging/bundle"
mkdir -p "${BUNDLE}/bin" "${BUNDLE}/share/komplement"
cp staging/komplement staging/konfigure "${BUNDLE}/bin/"
cp -r mappings "${BUNDLE}/share/komplement/"
tar -czf staging/payload.tar.gz -C "${BUNDLE}" .
PAYLOAD_BYTES="$(stat -c %s staging/payload.tar.gz)"
sed "s/@PAYLOAD_BYTES@/${PAYLOAD_BYTES}/" packaging/AppRun \
    > "dist/komplement-${VERSION}-${ARCH}.run"
cat staging/payload.tar.gz >> "dist/komplement-${VERSION}-${ARCH}.run"
chmod +x "dist/komplement-${VERSION}-${ARCH}.run"

echo
echo "Built:"
ls -1 dist/*.deb dist/*.rpm dist/*.run 2>/dev/null || true
echo
echo "To publish: ./packaging/release.sh ${VERSION}"
