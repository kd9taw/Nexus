#!/usr/bin/env bash
# Nexus — native Linux build (Ubuntu/Debian), producing a .deb + AppImage for SourceForge.
#
#   ./scripts/build-linux.sh            # UI + native modem + Tauri .deb/AppImage
#   ./scripts/build-linux.sh --no-gui   # native modem test exes only (fast)
#
# One-time dev deps (Ubuntu 24.04; the script checks and names anything missing):
#   sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev \
#     librsvg2-dev libxdo-dev libssl-dev libsoup-3.0-dev patchelf build-essential \
#     curl wget file cmake ninja-build gfortran nodejs npm
#   + rustup (https://rustup.rs); cargo-tauri is auto-installed if absent.
#
# Native build uses the SYSTEM FFTW3f (libfftw3f-dev) via pkg-config — no cross FFTW needed.
# CAT on Linux uses the system Hamlib: the .deb depends on libhamlib-utils (rigctld), and the
# AppImage falls back to `rigctld` on PATH, so AppImage users run `sudo apt install libhamlib-utils`.
set -euo pipefail

bold() { printf '\n\033[1m%s\033[0m\n' "$*"; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$*"; }
die()  { printf '\n\033[31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# Official-build secrets (e.g. CLUBLOG_API_KEY, baked via option_env!) live OUTSIDE the repo.
# shellcheck disable=SC1091
[ -f "$HOME/.nexus-build.env" ] && source "$HOME/.nexus-build.env"

GUI=1
for a in "$@"; do
  case "$a" in
    --no-gui) GUI=0 ;;
    -h|--help) sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $a" ;;
  esac
done

# 1 — toolchain + the GTK/WebKit dev libraries the native Tauri build links against ----------
bold "1/4  Toolchain + Linux GUI dev libraries"
miss=()
for t in cc gfortran cmake node npm; do command -v "$t" >/dev/null || miss+=("$t"); done
command -v ninja >/dev/null || command -v make >/dev/null || miss+=("ninja-or-make")
[ "$GUI" = 1 ] && { command -v patchelf >/dev/null || miss+=("patchelf"); }
[ "${#miss[@]}" -eq 0 ] || die "missing tools: ${miss[*]}
  Ubuntu/Debian: sudo apt install build-essential cmake ninja-build gfortran nodejs npm patchelf"
command -v cargo >/dev/null || die "Rust not found — install from https://rustup.rs"
pkg-config --exists fftw3f 2>/dev/null || die "libfftw3f-dev missing — sudo apt install libfftw3-dev"
if [ "$GUI" = 1 ]; then
  pcmiss=()
  for pc in webkit2gtk-4.1 gtk+-3.0 librsvg-2.0; do
    pkg-config --exists "$pc" 2>/dev/null || pcmiss+=("$pc")
  done
  [ "${#pcmiss[@]}" -eq 0 ] || die "missing GUI dev libraries (pkg-config): ${pcmiss[*]}
  Ubuntu/Debian: sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev libxdo-dev libssl-dev libsoup-3.0-dev"
fi
GEN=Ninja; command -v ninja >/dev/null || GEN="Unix Makefiles"
ok "cc/gfortran/cmake ($GEN)/node, system FFTW3f$([ "$GUI" = 1 ] && echo ', webkit2gtk-4.1, patchelf')"

# The DeepCW AI CW model (AGPL-3.0, (c) e04) is NOT committed — it is gitignored and staged
# into src-tauri/resources/deepcw by the caller (see that folder's README.md). Tauri's resource
# glob matches the directory whether or not the model is in it, so a missing model bundles a
# .deb that installs fine, runs fine, and silently has no AI CW decoder — 14 MB lighter with no
# error anywhere. A CI checkout has no way to obtain the file, so this is the DEFAULT there,
# not an edge case. Fail loudly instead of shipping a quietly-lobotomised build.
if [ "$GUI" = 1 ]; then
  dcw="$REPO/src-tauri/resources/deepcw"
  for f in model.onnx model.onnx.json; do
    [ -s "$dcw/$f" ] || die "missing $dcw/$f — the DeepCW model is gitignored and must be staged
  before bundling, or the build silently ships without the AI CW decoder.
  See src-tauri/resources/deepcw/README.md for provenance and how to fetch/fold it."
  done
  ok "DeepCW model staged ($(du -h "$dcw/model.onnx" | cut -f1))"
fi

# 2 — libtempo native modem test exes (proves the native chain; system FFTW3f via pkg-config) --
bold "2/4  libtempo native modem test exes"
# WX selects the WSJT-X-derived modem source. Unset (the normal case) means the in-tree
# vendored copy at libtempo/vendor/wsjtx. Export WX=/path/to/wsjtx-source to build against a
# different checkout; ft1-sys/build.rs reads the same variable, so both stay in step.
cmake -S "$REPO/libtempo" -B "$REPO/libtempo/build-linux" -G "$GEN" -DCMAKE_BUILD_TYPE=Release \
  ${WX:+-DWX="$WX"} >/dev/null
cmake --build "$REPO/libtempo/build-linux" >/dev/null
for e in tempodeep_test_standalone roundtrip tempofast_test_standalone acquire; do
  [ -f "$REPO/libtempo/build-linux/$e" ] && ok "$e" || warn "$e not produced"
done

if [ "$GUI" = 0 ]; then bold "Modem exes done (--no-gui)."; exit 0; fi

# 3 — UI build deps ---------------------------------------------------------------------------
bold "3/4  Web UI dependencies"
( cd "$REPO/ui" && npm ci >/dev/null )
ok "ui/node_modules"

# 4 — the GUI app + offline .deb + AppImage ---------------------------------------------------
bold "4/4  Nexus GUI app + .deb + AppImage"
cargo tauri --version >/dev/null 2>&1 || { warn "installing tauri-cli…"; cargo install tauri-cli --version "^2" --locked; }
[ -f "$REPO/src-tauri/icons/128x128.png" ] || python3 "$REPO/scripts/gen-icons.py"
# Linux uses the SYSTEM Hamlib (rigctld on PATH / the .deb's libhamlib-utils dependency), so DON'T
# ship the Windows hamlib .dll/.exe in the Linux bundle. The Windows build re-stages the real
# binaries via fetch-hamlib.sh, so removing them here is safe.
#
# Remove ONLY the untracked Windows binaries. This used to `rm -rf` the whole directory and
# recreate it with just a README — which DELETED four TRACKED Hamlib license files
# (AUTHORS/COPYING/COPYING.LIB/LICENSE) on every run, leaving the working tree dirty and, worse,
# one `git add -A` away from committing the removal of the license texts Hamlib's LGPL requires
# us to ship. The tracked README.txt is byte-identical to what that heredoc wrote, so nothing
# was gained by recreating it. Bit us for real on 2026-07-20.
find "$REPO/src-tauri/resources/hamlib" -type f \
  \( -name '*.dll' -o -name '*.exe' -o -name '*.lib' -o -name '*.def' \) -delete
# Safety net for the delete above: if it ever touches a TRACKED file (the LGPL license
# texts Hamlib requires us to ship), stop. Only meaningful inside a git work tree — the
# Pi Docker build COPYs the source WITHOUT .git (.dockerignore excludes it), so skip the
# check there rather than error on "not a git repository".
if git -C "$REPO" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
  git -C "$REPO" diff --quiet -- src-tauri/resources/hamlib || \
    die "build-linux.sh modified TRACKED files under src-tauri/resources/hamlib — refusing to
  continue. Those are the LGPL license texts Hamlib requires us to distribute; restore with
  'git checkout -- src-tauri/resources/hamlib/'."
fi
# The signature Tauri makes HERE is over bytes the repack below replaces, so it is deleted the
# moment the build finishes and remade at the end over the final file. `latest.json` is generated
# FROM the `.sig`, and a stale one does NOT fail anything: it ships, and every Linux self-update
# then fails verification. Deleting it is what makes that unrepresentable rather than merely
# avoided — after this point no `.sig` exists until the one made over the shipped bytes.
#
# ⚠️ DO NOT go back to withholding the key with `env -u` (which is what this file did until
# 1.8.0, and it broke the release build). Tauri treats "a pubkey is configured but no private key
# is present" as a FATAL error, not a warning — it bundles both artifacts and then exits 1 with
# "A public key has been found, but no private key". Every other platform job passes the key
# normally; the difference here was never intended.
( cd "$REPO/src-tauri" && cargo tauri build --features radio,custom-protocol --bundles deb,appimage )
# Kill the premature signature immediately — before the repack, so there is no window in which a
# stale one could be picked up by anything.
find "$REPO/src-tauri/target/release/bundle/appimage" -name '*.AppImage.sig' -delete 2>/dev/null || true
ok "Nexus .deb + AppImage"

# --- The Wayland client library has to come from the HOST (#138) --------------------------------
#
# linuxdeploy's GTK plugin copies GTK's dependencies wholesale into the AppDir, and that sweeps in
# `libwayland-client.so.0`. A Wayland client library talks a protocol version to the compositor it
# finds at runtime, so it must be the HOST's copy — which is why it is on AppImage's own
# excludelist (`AppImage/pkg2appimage/excludelist`), and the plugin's bulk copy bypasses that list.
#
# Shipping ours meant Nexus opened to a blank white window on Fedora 44 (#138, M0LHJ): the bundled
# Ubuntu-built copy loses to the host's newer compositor, graphics init fails, nothing is drawn.
# His own workaround is the proof of the mechanism —
# `LD_PRELOAD=/usr/lib64/libwayland-client.so.0` forces the system copy back in front and Nexus
# starts. Confusingly the app is not even a Wayland client: the AppRun forces `GDK_BACKEND=x11`,
# so this runs under XWayland and the library is still loaded further down the stack.
#
# ONLY that one library is removed. Its three siblings (`-egl`, `-cursor`, `-server`) are bundled
# too but are NOT on the excludelist, and nothing in the report points at them — removing them
# would be a guess, and a guess here is a build that fails to start for somebody else.
# ⚠️ AN ABSENT AppImage IS NORMAL, NOT AN ERROR. `scripts/Dockerfile.pi` seds the bundle list down
# to `--bundles deb` and runs this same script for both Raspberry Pi bases, so on those builds
# there is no AppImage to repack and nothing to sign. Failing here would take out both Pi jobs.
appimage=$(find "$REPO/src-tauri/target/release/bundle/appimage" -name '*.AppImage' -print -quit \
  2>/dev/null || true)
if [ -z "$appimage" ]; then
  ok "no AppImage in this bundle set (.deb-only build) — nothing to repack or sign"
else

work=$(mktemp -d)
( cd "$work" && "$appimage" --appimage-extract >/dev/null ) || die "could not unpack the AppImage"
if [ -e "$work/squashfs-root/usr/lib/libwayland-client.so.0" ]; then
  bold "Repacking the AppImage without the host-owned libwayland-client"
  rm -f "$work/squashfs-root/usr/lib/libwayland-client.so.0"
  # A positive control on the removal itself: if the file is still there the repack would ship the
  # bug while reporting success, which is the failure mode this whole section exists to prevent.
  [ ! -e "$work/squashfs-root/usr/lib/libwayland-client.so.0" ] \
    || die "libwayland-client.so.0 survived removal — refusing to repack"
  # Tauri already downloaded this during the bundle step, so the repack adds no new dependency and
  # uses the same packer that produced the original.
  packer="$HOME/.cache/tauri/linuxdeploy-plugin-appimage.AppImage"
  [ -x "$packer" ] || die "linuxdeploy's appimage plugin is not in ~/.cache/tauri — expected it
  there after 'cargo tauri build'. Set it executable, or rebuild so Tauri fetches it."
  ( cd "$work" && APPIMAGE_EXTRACT_AND_RUN=1 ARCH="${ARCH:-x86_64}" OUTPUT=repacked.AppImage \
      "$packer" --appdir squashfs-root >/dev/null 2>&1 ) \
    || die "repacking the AppImage failed"
  mv "$work/repacked.AppImage" "$appimage"
  chmod +x "$appimage"
  ok "libwayland-client.so.0 dropped; the host's copy is used instead"
else
  warn "libwayland-client.so.0 is not in this AppImage — nothing to strip (upstream may have"
  warn "fixed it). Leaving the bundle exactly as the bundler produced it."
fi
rm -rf "$work"

# --- Sign, now that the bytes are final ---------------------------------------------------------
# Unsigned when no key is present, which is every developer build and was already true before.
if [ -n "${TAURI_SIGNING_PRIVATE_KEY:-}" ]; then
  # It must not exist yet: the build's own signature was deleted above, so anything here would be
  # a signature over pre-repack bytes and every Linux self-update would fail verification.
  [ ! -e "$appimage.sig" ] \
    || die "a .sig already exists before signing — it would cover pre-repack bytes; refusing"
  ( cd "$REPO/src-tauri" && cargo tauri signer sign "$appimage" >/dev/null )
  [ -s "$appimage.sig" ] || die "signing produced no .sig beside the AppImage"
  # Newer than the file it signs, which is the only cheap check that catches a signature made
  # over the wrong bytes.
  [ "$appimage.sig" -nt "$appimage" ] \
    || die "the .sig is older than the AppImage it signs — refusing to publish it"
  ok "AppImage signed for self-update"
else
  warn "no TAURI_SIGNING_PRIVATE_KEY — AppImage published unsigned (developer build)"
fi

fi  # end: an AppImage was produced

bold "Done ✓  Linux artifacts:"
echo "  .deb     : src-tauri/target/release/bundle/deb/*.deb"
echo "  AppImage : src-tauri/target/release/bundle/appimage/*.AppImage"
echo "  binary   : src-tauri/target/release/nexus"
echo
warn "CAT needs Hamlib: the .deb pulls libhamlib-utils automatically; AppImage users run"
warn "'sudo apt install libhamlib-utils'. FT8/FT4 audio decode works without it (VOX)."
