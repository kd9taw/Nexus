#!/usr/bin/env bash
# Tempo — cross-build the Windows app from Linux / WSL2 (no MSYS2 needed).
#
# Uses the MinGW-w64 cross toolchain to produce Windows .exe files right here:
#   • src-tauri/target/x86_64-pc-windows-gnu/release/Nexus.exe        (the GUI app)
#   • target/x86_64-pc-windows-gnu/release/examples/win_smoke.exe     (static modem self-test)
#   • libtempo/build-win/{tempodeep_test_standalone,roundtrip,tempofast_test_standalone,acquire}.exe
#
#   ./scripts/build-windows-cross.sh            # everything (modem exes + GUI)
#   ./scripts/build-windows-cross.sh --modem    # only the modem exes (fast, fully static)
#   ./scripts/build-windows-cross.sh --no-gui   # modem + win_smoke, skip Nexus.exe
#   ./scripts/build-windows-cross.sh --bench-road-switch   # a TEST build for the bench, never shipped
#
# Prereqs (Debian/Ubuntu pkg names) — the script checks and reports what's missing:
#   gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 gfortran-mingw-w64-x86-64
#   cmake ninja-build nodejs npm  +  rustup  +  cc/make (to build FFTW and libvpx)
#   nasm (libvpx's x86 assembly)
set -euo pipefail

bold() { printf '\n\033[1m%s\033[0m\n' "$*"; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$*"; }
die()  { printf '\n\033[31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Official-build secrets (e.g. CLUBLOG_API_KEY, baked in via option_env!) live OUTSIDE the
# repo so they're never committed — ClubLog requires the key stay out of the public source.
# shellcheck disable=SC1091
[ -f "$HOME/.nexus-build.env" ] && source "$HOME/.nexus-build.env"
export PATH="$HOME/.local/bin:$PATH"     # picks up a pip-installed ninja, if any
TARGET=x86_64-pc-windows-gnu
FFTW_VER=3.3.10
export FFTW_MINGW_PREFIX="$REPO/target/fftw-mingw"   # read by ft1-sys/build.rs
VPX_VER=1.17.0
export VPX_MINGW_PREFIX="$REPO/target/vpx-mingw"     # read by crates/tempo-stream/build.rs

GUI=1; MODEM_ONLY=0; FEATURES=radio,custom-protocol
for a in "$@"; do
  case "$a" in
    --modem)  MODEM_ONLY=1 ;;
    --no-gui) GUI=0 ;;
    # A test build only: the switch that blocks one road to the shack at a time, for the bench.
    # Never a release: src-tauri's tests fail if a release workflow names it.
    --bench-road-switch) FEATURES+=,bench-road-switch ;;
    -h|--help) sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $a" ;;
  esac
done

# 1 — toolchain checks -------------------------------------------------------
bold "1/5  Cross toolchain"
miss=()
for t in x86_64-w64-mingw32-gcc x86_64-w64-mingw32-g++ x86_64-w64-mingw32-gfortran cmake nasm; do
  command -v "$t" >/dev/null || miss+=("$t")
done
command -v ninja >/dev/null || command -v make >/dev/null || miss+=("ninja-or-make")
[ "${#miss[@]}" -eq 0 ] || die "missing: ${miss[*]}
  Debian/Ubuntu: sudo apt install gcc-mingw-w64-x86-64 g++-mingw-w64-x86-64 gfortran-mingw-w64-x86-64 cmake ninja-build nasm"
command -v cargo >/dev/null || die "Rust not found — install from https://rustup.rs"
rustup target list --installed 2>/dev/null | grep -qx "$TARGET" || { warn "adding Rust target $TARGET"; rustup target add "$TARGET"; }
GEN=Ninja; command -v ninja >/dev/null || GEN="Unix Makefiles"
ok "mingw gcc/g++/gfortran, cmake ($GEN), Rust target $TARGET"

# 2 — FFTW3f for MinGW (built once, cached) ----------------------------------
bold "2/5  FFTW3f (single precision) for MinGW"
# SSE2/AVX/AVX2 codelets: WSJT-X's own Windows FFTW has SIMD (the DLL WSJT-X 2.7.0 installs
# reports fftw-3.3.10-sse2-avx). Built without them, FFTW 3.3.10 has 226 codelets and none SIMD;
# with them, 978 of which 752 are SIMD. Measured on one Windows PC, the plain build took 20-57 %
# longer over the early FT8 pass and 1-26 % longer over the full pass, for the same decodes. FFTW
# chooses codelets by CPUID at run time, so a CPU without AVX runs the SSE2 or plain ones.
# --with-our-malloc is FFTW's own aligned allocator: Windows' malloc gives 16 bytes of alignment,
# and AVX wants 32.
#
# The stamp records the version and these flags, so a cached build made with other flags (every
# CI cache and worktree from before SIMD) is rebuilt rather than linked.
fftw_cfg=(--host=x86_64-w64-mingw32 --enable-float --enable-static --disable-shared
  --enable-sse2 --enable-avx --enable-avx2 --with-our-malloc)
fftw_stamp="$FFTW_MINGW_PREFIX/.nexus-fftw"
if [ -f "$FFTW_MINGW_PREFIX/lib/libfftw3f.a" ] && [ "$(cat "$fftw_stamp" 2>/dev/null)" = "$FFTW_VER ${fftw_cfg[*]}" ]; then
  ok "cached at $FFTW_MINGW_PREFIX"
else
  rm -rf "$FFTW_MINGW_PREFIX"
  tmp="$(mktemp -d)"
  ( cd "$tmp"
    # ⚠️ HTTPS AND A PINNED HASH, because of WHAT this becomes. The `make` two lines below
    # executes whatever this tarball contains. release.yml runs it in a step of its own
    # (`--no-gui`) with no secret in its environment — but the library it builds is statically
    # linked into Nexus.exe, which is then legitimately SIGNED and published, and "Verify
    # installer" only checks a size floor. A tampered tarball ships. This used to be plain
    # `http://` with nothing to compare the bytes against, so an on-path attacker, a DNS spoof,
    # or a compromise of one academic web host was enough.
    #
    # The hash was confirmed against a SECOND party rather than just recomputed from what the
    # site served: Gentoo's sci-libs/fftw Manifest publishes size 4144100 and a SHA512 for
    # 3.3.10, and both match these bytes. Pinning a hash you only got from the host you are
    # trying not to trust proves nothing.
    #
    # Bump procedure: change FFTW_VER, download over HTTPS, verify the new bytes against an
    # independent packager (Gentoo's Manifest, Homebrew's formula), THEN update this line.
    url="https://www.fftw.org/fftw-${FFTW_VER}.tar.gz"
    fftw_sha256=56c932549852cddcfafdab3820b0200c7742675be92179e59e6215b340e26467
    (command -v curl >/dev/null && curl -fsSL -o fftw.tgz "$url") || wget -qO fftw.tgz "$url" \
      || die "could not download $url"
    echo "$fftw_sha256  fftw.tgz" | sha256sum -c - \
      || die "FFTW checksum mismatch — refusing to build. Someone changed the bytes at $url."
    tar xf fftw.tgz && cd "fftw-${FFTW_VER}"
    ./configure "${fftw_cfg[@]}" --prefix="$FFTW_MINGW_PREFIX" >/dev/null
    make -j"$(nproc)" >/dev/null && make install >/dev/null )
  rm -rf "$tmp"
  [ -f "$FFTW_MINGW_PREFIX/lib/libfftw3f.a" ] || die "FFTW cross-build failed"
  echo "$FFTW_VER ${fftw_cfg[*]}" > "$fftw_stamp"
  ok "built → $FFTW_MINGW_PREFIX"
fi

# 2b — libvpx (the VP8 encoder of Remote as a stream) for MinGW (built once, cached) --
bold "2b/5  libvpx (VP8 encoder) for MinGW"
# Pinned exactly as FFTW above is, and for the same reason: this runs, then links into Nexus.exe,
# inside the release step that holds the signing key. The hash is of the GitHub release archive
# for the tag, and was confirmed against TWO independent packagers rather than recomputed from
# what GitHub served: Homebrew's libvpx formula and FreeBSD ports' multimedia/libvpx distinfo
# both publish this SHA-256 for v1.17.0 (FreeBSD also the size, 5670546 bytes). Bump the same
# way: new VPX_VER, download over HTTPS, check the bytes against two packagers, THEN this line.
#
# What is built: the VP8 encoder only (no decoder, no VP9, no tools), realtime-only, static,
# single-threaded (so no winpthread DLL), SIMD chosen at run time. The stamp file records the
# version and hash, so a cached build of an older release is rebuilt rather than linked.
vpx_sha256=1020f184046187baa2985dbde38e0691f49c44088bca7a1842b0236c6081dc0a
vpx_stamp="$VPX_MINGW_PREFIX/.nexus-libvpx"
if [ -f "$VPX_MINGW_PREFIX/lib/libvpx.a" ] && [ "$(cat "$vpx_stamp" 2>/dev/null)" = "$VPX_VER $vpx_sha256" ]; then
  ok "cached at $VPX_MINGW_PREFIX (libvpx $VPX_VER)"
else
  rm -rf "$VPX_MINGW_PREFIX"
  VPX_LOG="$REPO/target/vpx-mingw-build.log"
  mkdir -p "$REPO/target"
  tmp="$(mktemp -d)"
  if ! ( cd "$tmp"
    url="https://github.com/webmproject/libvpx/archive/refs/tags/v${VPX_VER}.tar.gz"
    (command -v curl >/dev/null && curl -fsSL -o libvpx.tgz "$url") || wget -qO libvpx.tgz "$url" \
      || { echo "could not download $url"; exit 1; }
    echo "$vpx_sha256  libvpx.tgz" | sha256sum -c - \
      || { echo "libvpx checksum mismatch — refusing to build. Someone changed the bytes at $url."; exit 1; }
    # One && chain: `set -e` does not reach inside an `if` condition.
    tar xf libvpx.tgz && mkdir build && cd build \
      && CROSS=x86_64-w64-mingw32- "../libvpx-${VPX_VER}/configure" --target=x86_64-win64-gcc \
        --prefix="$VPX_MINGW_PREFIX" --as=nasm --enable-static --disable-shared \
        --disable-examples --disable-tools --disable-docs --disable-unit-tests \
        --disable-install-docs --disable-install-bins --disable-install-srcs \
        --disable-vp9 --disable-vp8-decoder --enable-vp8-encoder --enable-realtime-only \
        --enable-runtime-cpu-detect --disable-multithread --disable-webm-io --disable-libyuv \
      && make -j"$(nproc)" && make install ) >"$VPX_LOG" 2>&1; then
    rm -rf "$tmp"
    tail -n 40 "$VPX_LOG"; die "libvpx cross-build failed (last 40 lines of $VPX_LOG above)"
  fi
  rm -rf "$tmp"
  [ -f "$VPX_MINGW_PREFIX/lib/libvpx.a" ] || die "libvpx cross-build produced no libvpx.a"
  echo "$VPX_VER $vpx_sha256" > "$vpx_stamp"
  ok "built → $VPX_MINGW_PREFIX (libvpx $VPX_VER)"
fi

# BUILD PATHS: from here on, every compiler maps $HOME to `~`. Each one embeds the absolute path
# of what it compiles: rustc for panic locations (crates.io sources included), gcc for __FILE__,
# gfortran for its run-time and I/O error messages. Built under a home directory, Nexus.exe carried
# that path, and with it the account name, more than a thousand times; CI's installers only ever
# held its runner's. Not set before FFTW and libvpx: FFTW's configure leaves out its own -O3 when
# CFLAGS is set, and both build in a temporary directory, so neither embeds a home path anyway.
#   • rustc: --remap-path-prefix in RUSTFLAGS. RUSTFLAGS REPLACES `rustflags` from a cargo config
#     file rather than adding to it; this repo sets none there.
#   • gcc (the cc and cmake crates, and CMake's C/C++): -ffile-prefix-map in CFLAGS/CXXFLAGS.
#   • gfortran ignores -ffile-prefix-map for those paths, so scripts/fortran-remap-home.sh rewrites
#     them where gfortran reads them, as CMake's Fortran compiler launcher.
# CMake reads CFLAGS and the launcher only when it first configures a build directory. That is
# enough: a changed RUSTFLAGS gives every build script a new OUT_DIR, so the libtempo cargo links
# is configured afresh the first time this runs in an existing tree.
export RUSTFLAGS="${RUSTFLAGS:+$RUSTFLAGS }--remap-path-prefix=$HOME=~"
export CFLAGS="${CFLAGS:+$CFLAGS }-ffile-prefix-map=$HOME=~"
export CXXFLAGS="${CXXFLAGS:+$CXXFLAGS }-ffile-prefix-map=$HOME=~"
export CMAKE_Fortran_COMPILER_LAUNCHER="$REPO/scripts/fortran-remap-home.sh${CMAKE_Fortran_COMPILER_LAUNCHER:+;$CMAKE_Fortran_COMPILER_LAUNCHER}"

# 3 — libtempo modem test exes (proves the native chain on Windows) ------------
bold "3/5  libtempo Windows test exes"
# Configure/build output goes to a log; on FAILURE the log is dumped so CI and
# operators see the real error (a bare >/dev/null swallowed ninja/CMake errors).
LIBFT1_LOG="$REPO/libtempo/build-win-configure.log"
if ! cmake -S "$REPO/libtempo" -B "$REPO/libtempo/build-win" -G "$GEN" \
  -DCMAKE_TOOLCHAIN_FILE="$REPO/libtempo/mingw-w64.cmake" \
  -DFFTW_MINGW_PREFIX="$FFTW_MINGW_PREFIX" -DCMAKE_BUILD_TYPE=Release >"$LIBFT1_LOG" 2>&1; then
  cat "$LIBFT1_LOG"; die "libtempo CMake configure failed (log above)"
fi
if ! cmake --build "$REPO/libtempo/build-win" >"$LIBFT1_LOG" 2>&1; then
  tail -n 80 "$LIBFT1_LOG"; die "libtempo build failed (last 80 lines above)"
fi
for e in tempodeep_test_standalone roundtrip tempofast_test_standalone acquire; do
  [ -f "$REPO/libtempo/build-win/$e.exe" ] && ok "$e.exe" || warn "$e.exe not produced"
done

# 4 — win_smoke.exe : FT1 + DX1 round-trip, fully static (no DLLs) -----------
bold "4/5  Rust modem self-test (win_smoke.exe)"
cargo build --release --target "$TARGET" -p tempo-fast --example win_smoke
ok "target/$TARGET/release/examples/win_smoke.exe"

if [ "$MODEM_ONLY" = 1 ]; then bold "Modem exes done (--modem)."; exit 0; fi

# 5 — the GUI app + offline installer (tempo.exe + NSIS) ---------------------
if [ "$GUI" = 1 ]; then
  bold "5/5  GUI app + installer (Nexus.exe, NSIS)"
  command -v npm >/dev/null || die "npm not found — install Node.js LTS to build the UI."
  command -v makensis >/dev/null || warn "makensis not found — the NSIS step needs it (Debian/Ubuntu: sudo apt install nsis)."
  cargo tauri --version >/dev/null 2>&1 || { warn "installing tauri-cli…"; cargo install tauri-cli --version "=2.11.5" --locked; }
  ( cd "$REPO/ui" && npm ci >/dev/null )            # deps; cargo tauri runs the build
  [ -f "$REPO/src-tauri/icons/icon.ico" ] || python3 "$REPO/scripts/gen-icons.py"
  bash "$REPO/scripts/fetch-hamlib.sh"              # bundle Hamlib for CAT (no-op if staged)

  # Remove the UNIX Hamlib binaries, the mirror of what build-linux.sh does for the Windows
  # ones. `src-tauri/resources/hamlib/` is ONE staging directory shared by every target and
  # tauri.conf.json globs it whole (`resources/hamlib/*`), so whichever platform builds SECOND
  # ships both. build-linux.sh has always deleted *.dll/*.exe; nothing did the reverse, so a
  # Linux-then-Windows build put 12.6 MB of ELF binaries inside the installer — files that can
  # never run there. CI never saw it because it builds each platform on its own runner; it only
  # bites a local build of both, which is the normal way to cut a tester pair.
  # Extension-less here means an ELF executable (rigctl/rigctld/rotctl/rotctld). The tracked
  # LGPL texts are all *.txt and are not matched.
  find "$REPO/src-tauri/resources/hamlib" -type f \
    \( -name '*.so' -o -name '*.so.*' -o ! -name '*.*' \) -delete
  # Same safety net build-linux.sh carries: if that delete ever reaches a TRACKED file — the
  # licence texts Hamlib's LGPL requires us to ship — stop rather than continue.
  if git -C "$REPO" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
    git -C "$REPO" diff --quiet -- src-tauri/resources/hamlib || \
      die "build-windows-cross.sh modified TRACKED files under src-tauri/resources/hamlib —
  refusing to continue. Restore with 'git checkout -- src-tauri/resources/hamlib/'."
  fi
  # THE SILENT-LOBOTOMY GUARD. The DeepCW engine is a PAIR of gitignored files
  # (weights + metadata sidecar), so a fresh clone — and every git WORKTREE,
  # which checks out tracked files only — builds an installer whose AI CW
  # decoder can never load, with no build-time sign of it. Shipped once exactly
  # that way (0.21.6: model.onnx staged, model.onnx.json forgotten -> "model not
  # installed" on the operator's rig). Fail here instead.
  DEEPCW="$REPO/src-tauri/resources/deepcw"
  for f in model.onnx model.onnx.json; do
    if [ ! -s "$DEEPCW/$f" ]; then
      warn "stage BOTH halves from a checkout that has them:"
      warn "  cp <checkout>/src-tauri/resources/deepcw/model.onnx.json $DEEPCW/"
      die "DeepCW resource missing or empty: $DEEPCW/$f"
    fi
  done
  ok "DeepCW engine staged (weights + metadata)"
  # The IOTA island-group list ships in the installer by IOTA Ltd's permission but is never
  # committed (src-tauri/resources/iota/README.md), so a checkout has none. Fetch it when it is
  # not already staged (release.yml stages its own). Best-effort, unlike DeepCW: a build without
  # it still works, checking island references by shape until the app's first monthly download.
  if [ ! -s "$REPO/src-tauri/resources/iota/iota_groups.json" ]; then
    if node "$REPO/scripts/gen-iota-groups.mjs" --download "$REPO/src-tauri/resources/iota"; then
      ok "IOTA island list staged"
    else
      warn "IOTA island list NOT staged — this installer ships without it"
    fi
  else
    ok "IOTA island list already staged"
  fi
  # cargo tauri build enables asset embedding (custom-protocol — the fix for the
  # blank "page cannot be displayed" screen) and bundles the offline installer.
  # With no updater key in the environment it builds with --no-sign, because Tauri treats
  # "a pubkey is configured but no private key" as fatal. That is how release.yml runs this:
  # its build jobs never see the key, and a separate job signs the finished installer. A local
  # build with the key in ~/.nexus-build.env still signs here, as before.
  no_sign=(); [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ] || no_sign=(--no-sign)
  ( cd "$REPO/src-tauri" && cargo tauri build --target "$TARGET" --features "$FEATURES" --bundles nsis "${no_sign[@]}" )
  ok "Nexus.exe + installer"
fi

bold "Done ✓  Windows artifacts:"
echo "  installer (run this): src-tauri/target/$TARGET/release/bundle/nsis/Nexus_*_x64-setup.exe"
echo "  GUI app             : src-tauri/target/$TARGET/release/Nexus.exe"
echo "  modem self-test     : target/$TARGET/release/examples/win_smoke.exe   (fully static — runs anywhere)"
echo "  modem test exes     : libtempo/build-win/*.exe"
echo
warn "Cross-compiled, NOT run here — smoke-test on Windows. The installer bundles"
warn "the offline WebView2 runtime + Hamlib (rigctld), so it installs clean & CAT works."
