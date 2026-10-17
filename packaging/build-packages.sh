#!/usr/bin/env bash
# Everything the release carries, built here and nowhere else:
#
#   dist/komplement_<version>_<arch>.deb
#   dist/komplement-<version>.<arch>.rpm
#   dist/komplement-<version>-<arch>.AppImage
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

# The AppImage: the binaries, the mappings, and an AppRun that knows both.
APPDIR="${PWD}/staging/komplement.AppDir"
mkdir -p "${APPDIR}/usr/bin" "${APPDIR}/usr/share/komplement"
cp staging/komplement staging/konfigure "${APPDIR}/usr/bin/"
cp -r mappings "${APPDIR}/usr/share/komplement/"
cp packaging/AppRun "${APPDIR}/AppRun"
chmod +x "${APPDIR}/AppRun"
cat > "${APPDIR}/komplement.desktop" <<'EOF'
[Desktop Entry]
Type=Application
Name=komplement
Comment=Komplete Kontrol A-series control surface
Exec=komplement
Categories=Audio;Music;
Terminal=true
EOF
nix shell nixpkgs#appimagekit --command \
    appimagetool "${APPDIR}" "dist/komplement-${VERSION}-${ARCH}.AppImage"

echo
echo "Built:"
ls -1 dist/*.deb dist/*.rpm dist/*.AppImage 2>/dev/null || true
echo
echo "To publish: ./packaging/release.sh ${VERSION}"
