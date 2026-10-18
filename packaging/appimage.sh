#!/usr/bin/env bash
# Builds a real AppImage, which needs a world that is not Nix: linuxdeploy and
# appimagetool both run ldd, and ldd wants an ordinary /usr. On NixOS that world
# is buildFHSEnv, and this script makes one, borrows it, and leaves it behind.
#
#   ./packaging/appimage.sh 0.1.0
#
# Verified as far as it goes without the last step: the FHS environment is what
# makes ldd work (measured - it copies libudev and libhidapi-libusb out of the
# store), binutils has to be in it or linuxdeploy fails at strip, and the
# desktop file needs an icon beside it. The packing at the end uses appimagetool
# from the AppImage project's own releases, because nixpkgs no longer carries
# appimagetool or appimagekit; that step has not been run here.
set -euo pipefail

cd "$(dirname "$0")/.."

VERSION="${1:-$(git describe --tags --always --dirty 2>/dev/null || echo 0.0.0)}"
VERSION="${VERSION#v}"
case "$(uname -m)" in
    x86_64) ARCH=x86_64 ;;
    aarch64) ARCH=aarch64 ;;
    *) echo "unsupported machine: $(uname -m)" >&2; exit 1 ;;
esac
echo "komplement ${VERSION} for ${ARCH}"

# The binaries, out of the flake, as everywhere else.
rm -rf dist staging
mkdir -p dist staging
STORE="$(nix build --no-link --print-out-paths .#default | head -1)"
cp "${STORE}/bin/komplement" "${STORE}/bin/konfigure" staging/
chmod +x staging/*

# appimagetool, from AppImage's own releases. It is itself an AppImage, so it is
# run with APPIMAGE_EXTRACT_AND_RUN=1 and never needs FUSE.
TOOL="staging/appimagetool-${ARCH}.AppImage"
if [ ! -x "${TOOL}" ]; then
    echo "Fetching appimagetool"
    nix shell nixpkgs#curl --command curl -fsSL -o "${TOOL}" \
        "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-${ARCH}.AppImage"
    chmod +x "${TOOL}"
fi

# The icon. Put a real one at packaging/komplement.png; without one, this is a
# plain placeholder so that linuxdeploy has something to accept.
ICON="packaging/komplement.png"
if [ ! -e "${ICON}" ]; then
    nix shell nixpkgs#imagemagick --command magick -size 256x256 xc:none \
        -fill '#2b6cb0' -draw 'roundrectangle 8,8 248,248 48,48' \
        -fill white -draw 'circle 128,128 128,58' "${ICON}"
fi

# The AppDir: both binaries, the mappings, the desktop entry, the AppRun.
APPDIR="${PWD}/staging/komplement.AppDir"
rm -rf "${APPDIR}"
mkdir -p "${APPDIR}/usr/bin" "${APPDIR}/usr/share/komplement" \
         "${APPDIR}/usr/share/applications" "${APPDIR}/usr/share/icons/hicolor/256x256/apps"
cp staging/komplement staging/konfigure "${APPDIR}/usr/bin/"
cp -r mappings "${APPDIR}/usr/share/komplement/"
cp packaging/appimage/AppRun "${APPDIR}/AppRun"
chmod +x "${APPDIR}/AppRun"
cp packaging/appimage/komplement.desktop "${APPDIR}/komplement.desktop"
cp packaging/appimage/komplement.desktop "${APPDIR}/usr/share/applications/"
cp "${ICON}" "${APPDIR}/komplement.png"
cp "${ICON}" "${APPDIR}/usr/share/icons/hicolor/256x256/apps/komplement.png"

# The libraries and the rpath that finds them - the part that needs a normal
# /usr, and so the part that needs the FHS environment. NO --output here: the
# appimage plugin linuxdeploy would call is not in nixpkgs any more.
echo "Borrowing an FHS environment for linuxdeploy"
NIXPKGS_EXPR='
let pkgs = import (builtins.getFlake "nixpkgs") {}; in
pkgs.buildFHSEnv {
  name = "komplement-appimage-fhs";
  targetPkgs = p: with p; [
    linuxdeploy binutils glibc.bin file patchelf coreutils which
    desktop-file-utils zlib libgcc libusb1 alsa-lib hidapi systemd
  ];
  runScript = "bash";
}'
nix-build --impure --no-out-link -o staging/fhs --expr "${NIXPKGS_EXPR}" >/dev/null
FHS="$(readlink -f staging/fhs)"

"${FHS}/bin/komplement-appimage-fhs" -c "
set -e
cd '${PWD}'
linuxdeploy --appdir '${APPDIR}' \
    --desktop-file '${APPDIR}/komplement.desktop' \
    --icon-file '${APPDIR}/komplement.png'
"

echo "Packing the AppImage"
out="dist/komplement-${VERSION}-${ARCH}.AppImage"
ARCH="${ARCH}" APPIMAGE_EXTRACT_AND_RUN=1 \
    "${PWD}/${TOOL}" --no-appstream "${APPDIR}" "${out}"

ls -l "${out}"
echo
echo "Run it anywhere:            ${PWD}/${out}"
echo "Without FUSE (no kernel support needed):"
echo "    APPIMAGE_EXTRACT_AND_RUN=1 ${PWD}/${out}"
