#!/usr/bin/env bash
# Nexus — native macOS build, producing an unsigned Nexus.app bundle.
#
#   ./scripts/build-macos.sh                 # UI + modem + Nexus.app
#   ./scripts/build-macos.sh --allow-missing-aicw
#   ./scripts/build-macos.sh --no-gui         # native modem CMake check only
#
# One-time deps (Homebrew):
#   brew install cmake ninja gcc fftw boost pkgconf
#   + Xcode CLT, rustup, Node.js; cargo-tauri is auto-installed if absent.
#   CAT: brew install hamlib  (rigctld on PATH — not bundled in the .app)
#
# This is the community / from-source path. Official signed+notarized macOS
# packages are a separate maintainer decision (Apple Developer ID). Unsigned
# builds are expected to trip Gatekeeper (right-click → Open), same class of
# warning as the unsigned Windows installer and SmartScreen.
set -euo pipefail

bold() { printf '\n\033[1m%s\033[0m\n' "$*"; }
ok()   { printf '  \033[32m✓\033[0m %s\n' "$*"; }
warn() { printf '  \033[33m!\033[0m %s\n' "$*"; }
die()  { printf '\n\033[31m✗ %s\033[0m\n' "$*" >&2; exit 1; }

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# shellcheck disable=SC1091
[ -f "$HOME/.nexus-build.env" ] && source "$HOME/.nexus-build.env"

GUI=1
ALLOW_MISSING_AICW=0
for a in "$@"; do
  case "$a" in
    --no-gui) GUI=0 ;;
    --allow-missing-aicw) ALLOW_MISSING_AICW=1 ;;
    -h|--help) sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'; exit 0 ;;
    *) die "unknown option: $a" ;;
  esac
done

command -v brew >/dev/null || die "Homebrew not found — install from https://brew.sh"

# 1 — toolchain --------------------------------------------------------------------------------
bold "1/4  Toolchain + Homebrew modem libraries"
miss=()
for t in cc gfortran cmake ninja node npm pkg-config; do
  command -v "$t" >/dev/null || miss+=("$t")
done
command -v cargo >/dev/null || die "Rust not found — install from https://rustup.rs"
[ "${#miss[@]}" -eq 0 ] || die "missing tools: ${miss[*]}
  brew install cmake ninja gcc fftw boost pkgconf
  (+ Node.js LTS from https://nodejs.org/)"

pkg-config --exists fftw3f 2>/dev/null || die "fftw (single-precision) missing — brew install fftw"
# Homebrew's libgfortran lives under the gcc formula, not on the default linker path.
export LIBRARY_PATH="$(brew --prefix gcc)/lib/gcc/current:$(brew --prefix fftw)/lib:${LIBRARY_PATH:-}"
export PKG_CONFIG_PATH="$(brew --prefix fftw)/lib/pkgconfig:$(brew --prefix boost)/lib/pkgconfig:${PKG_CONFIG_PATH:-}"
ok "cc/gfortran/cmake/ninja/node, Homebrew FFTW3f + libgfortran on LIBRARY_PATH"

# DeepCW model — same contract as build-linux.sh. Absent model ⇒ silent lobotomy of AI CW.
if [ "$GUI" = 1 ]; then
  dcw="$REPO/src-tauri/resources/deepcw"
  missing=0
  for f in model.onnx model.onnx.json; do
    [ -s "$dcw/$f" ] || missing=1
  done
  if [ "$missing" = 1 ]; then
    if [ "$ALLOW_MISSING_AICW" = 1 ] || [ "${NEXUS_ALLOW_MISSING_AICW:-}" = 1 ]; then
      warn "DeepCW model missing — continuing with NEXUS_ALLOW_MISSING_AICW (no AI CW decoder)"
      export NEXUS_ALLOW_MISSING_AICW=1
    else
      die "missing $dcw/model.onnx (and/or model.onnx.json) — stage the DeepCW model
  before bundling, or pass --allow-missing-aicw / NEXUS_ALLOW_MISSING_AICW=1.
  See src-tauri/resources/deepcw/README.md."
    fi
  else
    ok "DeepCW model staged ($(du -h "$dcw/model.onnx" | cut -f1))"
  fi
fi

# 2 — libtempo native modem (proves the Fortran/CMake chain) -----------------------------------
bold "2/4  libtempo native modem (CMake)"
cmake -S "$REPO/libtempo" -B "$REPO/libtempo/build-macos" -G Ninja -DCMAKE_BUILD_TYPE=Release \
  ${WX:+-DWX="$WX"} >/dev/null
cmake --build "$REPO/libtempo/build-macos" >/dev/null
ok "libtempo/build-macos"

if [ "$GUI" = 0 ]; then bold "Modem build done (--no-gui)."; exit 0; fi

# 3 — UI ---------------------------------------------------------------------------------------
bold "3/4  Web UI dependencies"
( cd "$REPO/ui" && npm install >/dev/null )
ok "ui/node_modules"

# 4 — Tauri .app -------------------------------------------------------------------------------
bold "4/4  Nexus.app (unsigned)"
cargo tauri --version >/dev/null 2>&1 || {
  warn "installing tauri-cli…"
  cargo install tauri-cli --version "^2" --locked
}
[ -f "$REPO/src-tauri/icons/128x128.png" ] || python3 "$REPO/scripts/gen-icons.py"

# With a pubkey configured, Tauri hard-errors if createUpdaterArtifacts is true and no
# private signing key is present (same trap as the Pi Docker build). Flip it off for this
# local run only; restore on exit so the working tree stays clean.
CONF="$REPO/src-tauri/tauri.conf.json"
CONF_BAK="$(mktemp)"
cp "$CONF" "$CONF_BAK"
restore_conf() { cp "$CONF_BAK" "$CONF"; rm -f "$CONF_BAK"; }
trap restore_conf EXIT
python3 - "$CONF" <<'PY'
import json, sys
p = sys.argv[1]
with open(p) as f:
    c = json.load(f)
c.setdefault("bundle", {})["createUpdaterArtifacts"] = False
with open(p, "w") as f:
    json.dump(c, f, indent=2)
    f.write("\n")
PY

( cd "$REPO/src-tauri" && cargo tauri build --features radio,custom-protocol --bundles app )
ok "Nexus.app"

bold "Done ✓  macOS artifact:"
echo "  app : src-tauri/target/release/bundle/macos/Nexus.app"
echo
warn "Unsigned build — Gatekeeper: right-click Nexus.app → Open."
warn "CAT needs Hamlib on PATH: brew install hamlib  (rigctld)."
