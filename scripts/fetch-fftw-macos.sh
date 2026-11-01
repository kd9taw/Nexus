#!/usr/bin/env bash
# Build FFTW3f (single precision, static) FROM SOURCE for the macOS build, at the app's minimum
# macOS, for the libtempo modem to link statically into Nexus.
#
# WHY NOT HOMEBREW'S. tempo-fast-sys links libfftw3f.a statically on macOS (see its build.rs: a
# dynamic Homebrew FFTW ships an app that dyld kills on a Mac without Homebrew). A Homebrew bottle
# is built for the runner's macOS, 14 or 15, so every FFTW object linked into an app that promises
# macOS 12 was built for a newer system than the app says it needs; the linker only warns. This
# builds the same pinned FFTW the Windows build uses (scripts/build-windows-cross.sh), at
# MACOSX_DEPLOYMENT_TARGET = bundle.macOS.minimumSystemVersion from src-tauri/tauri.conf.json.
#
# SIMD, as Homebrew builds it for each architecture, so the modem's FFTs run as they did with the
# bottle: SSE2/AVX/AVX2 codelets on Intel (FFTW picks by CPUID at run time, so a CPU without AVX
# runs the SSE2 or plain ones), and on Apple Silicon the ARMv8 cycle counter FFTW's planner times
# with. Licence: FFTW is GPL-2.0-or-later, already shipped in every Nexus build (NOTICE).
#
#   ./scripts/fetch-fftw-macos.sh        # builds once into target/fftw-macos (or $FFTW_MACOS_PREFIX)
#   PKG_CONFIG_PATH=<prefix>/lib/pkgconfig is what the cargo build then needs: CMake's
#   pkg_check_modules(fftw3f) and build.rs's pkg-config probe both find it there.
set -euo pipefail

FFTW_VER=3.3.10
# The same pin as build-windows-cross.sh, confirmed there against Gentoo's Manifest (a second
# party). Move both together.
FFTW_SHA256=56c932549852cddcfafdab3820b0200c7742675be92179e59e6215b340e26467
URL="https://www.fftw.org/fftw-${FFTW_VER}.tar.gz"

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PREFIX="${FFTW_MACOS_PREFIX:-$REPO/target/fftw-macos}"

[ "$(uname -s)" = Darwin ] || { echo "fetch-fftw-macos.sh is for macOS" >&2; exit 1; }
for tool in make cc python3 pkg-config; do
  command -v "$tool" >/dev/null || { echo "need $tool to build FFTW" >&2; exit 1; }
done

# The minimum macOS, from the one place it is set; a deployment target already in the
# environment must agree with it (as in fetch-hamlib-unix.sh).
floor=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["bundle"]["macOS"]["minimumSystemVersion"])' \
          "$REPO/src-tauri/tauri.conf.json") \
  || { echo "src-tauri/tauri.conf.json sets no bundle.macOS.minimumSystemVersion" >&2; exit 1; }
if [ -n "${MACOSX_DEPLOYMENT_TARGET:-}" ] && [ "$MACOSX_DEPLOYMENT_TARGET" != "$floor" ]; then
  echo "MACOSX_DEPLOYMENT_TARGET=$MACOSX_DEPLOYMENT_TARGET, but tauri.conf.json's minimum is $floor" >&2; exit 1
fi
export MACOSX_DEPLOYMENT_TARGET="$floor"

ARCH="$(uname -m)"
cfg=(--enable-float --enable-static --disable-shared)
case "$ARCH" in
  x86_64) cfg+=(--enable-sse2 --enable-avx --enable-avx2) ;;
  arm64)  cfg+=(--enable-armv8-cntvct-el0) ;;
  *) echo "unexpected macOS architecture $ARCH" >&2; exit 1 ;;
esac

# The stamp records the version, flags, minimum and architecture, so a build made for anything
# else is rebuilt rather than linked.
stamp="$PREFIX/.nexus-fftw"
want="$FFTW_VER ${cfg[*]} macos-$MACOSX_DEPLOYMENT_TARGET $ARCH"
if [ -f "$PREFIX/lib/libfftw3f.a" ] && [ "$(cat "$stamp" 2>/dev/null)" = "$want" ]; then
  echo "FFTW $FFTW_VER already built at $PREFIX ($ARCH, macOS $MACOSX_DEPLOYMENT_TARGET)"
else
  rm -rf "$PREFIX"
  tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
  echo "Building FFTW $FFTW_VER ($ARCH, macOS $MACOSX_DEPLOYMENT_TARGET): ${cfg[*]}"
  curl -fsSL -o "$tmp/fftw.tgz" "$URL"
  echo "$FFTW_SHA256  $tmp/fftw.tgz" | shasum -a 256 -c - \
    || { echo "FFTW checksum mismatch — refusing to build. Someone changed the bytes at $URL." >&2; exit 1; }
  tar xzf "$tmp/fftw.tgz" -C "$tmp"
  ( cd "$tmp/fftw-${FFTW_VER}" && ./configure "${cfg[@]}" --prefix="$PREFIX" >"$tmp/build.log" 2>&1 \
      && make -j"$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)" >>"$tmp/build.log" 2>&1 \
      && make install >>"$tmp/build.log" 2>&1 ) \
    || { echo "FFTW build FAILED" >&2; tail -25 "$tmp/build.log" >&2; exit 1; }
  echo "$want" > "$stamp"
fi

# What the modem build will find: this prefix, first on the pkg-config path, and its Fortran
# interface beside the C header (four2a.f90 includes fftw3.f03).
for f in lib/libfftw3f.a include/fftw3.h include/fftw3.f03 lib/pkgconfig/fftw3f.pc; do
  [ -s "$PREFIX/$f" ] || { echo "FFTW build is missing $PREFIX/$f" >&2; exit 1; }
done
got=$(PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}" pkg-config --variable=libdir fftw3f)
[ "$got" = "$PREFIX/lib" ] || { echo "pkg-config resolves fftw3f to $got, not $PREFIX/lib" >&2; exit 1; }
echo "FFTW $FFTW_VER staged → $PREFIX (set PKG_CONFIG_PATH=$PREFIX/lib/pkgconfig)"
