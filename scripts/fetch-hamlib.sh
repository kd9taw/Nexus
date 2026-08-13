#!/usr/bin/env bash
# Fetch the Hamlib (rigctld) Windows runtime and stage it as a Tauri bundle
# resource at src-tauri/resources/hamlib/, so the Windows installer ships CAT
# rig control with zero extra installs. Hamlib is GPL/LGPL — compatible with
# Tempo's GPLv3. The binaries are NOT committed (see .gitignore); this script
# reproduces them.
#
#   ./scripts/fetch-hamlib.sh        # idempotent; skips if already staged
set -euo pipefail

VER=4.7.1
ZIP="hamlib-w64-${VER}.zip"
URL="https://github.com/Hamlib/Hamlib/releases/download/${VER}/${ZIP}"
SHA256=5b2a5d6efc37171c24ee6ac44e6304710219859f30fa4dfc77688f71b3440402

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$REPO/src-tauri/resources/hamlib"
# What rigctld.exe needs at runtime (+ rigctl.exe for `rigctl -l`, + licenses).
WANT=(rigctld.exe rigctl.exe rotctld.exe rotctl.exe libhamlib-4.dll libwinpthread-1.dll libusb-1.0.dll libgcc_s_seh-1.dll)
LIC=(COPYING.txt COPYING.LIB.txt LICENSE.txt AUTHORS.txt)
# The smallest real binary in the 4.7.1 zip is rotctl.exe at ~114 KB, so this
# floor cleanly separates a staged binary from an empty or truncated one. The
# zip's sha256 above already covers subtly-WRONG content; this covers ABSENT.
MIN_BYTES=32768

# Names every WANT file that is absent or implausibly small; silence = staged.
# Used BOTH as the "already staged?" test and as the post-condition after the
# copy, because the copy loop cannot report its own failure: `cp … && echo …`
# is a && list, so set -e does NOT fire when the cp fails. A changed archive
# layout therefore copies NOTHING, prints "Hamlib staged", and exits 0 — and
# the installer ships a hamlib/ directory holding only the licence .txt files.
gaps() {
  local f n
  for f in "${WANT[@]}"; do
    if [ ! -f "$DEST/$f" ]; then
      echo "  $f — MISSING"
    else
      n=$(wc -c <"$DEST/$f")
      [ "$n" -ge "$MIN_BYTES" ] || echo "  $f — only $n bytes (truncated?)"
    fi
  done
}

if [ -z "$(gaps)" ]; then
  echo "Hamlib already staged at $DEST"; exit 0
fi

tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
echo "Downloading Hamlib $VER…"
(command -v curl >/dev/null && curl -fsSL -o "$tmp/$ZIP" "$URL") || wget -qO "$tmp/$ZIP" "$URL"
echo "$SHA256  $tmp/$ZIP" | sha256sum -c - || { echo "checksum mismatch — aborting"; exit 1; }

if command -v unzip >/dev/null; then
  unzip -qo "$tmp/$ZIP" -d "$tmp"
else
  python3 -c "import zipfile,sys; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])" "$tmp/$ZIP" "$tmp"
fi

root="$tmp/hamlib-w64-${VER}"
mkdir -p "$DEST"
for f in "${WANT[@]}"; do cp "$root/bin/$f" "$DEST/" && echo "  + $f"; done
for f in "${LIC[@]}";  do cp "$root/$f"     "$DEST/" 2>/dev/null || true; done

# THE CAT-DEAD GUARD. Everything above can fail without failing the script (see
# gaps() ), and the result is invisible until an operator's rig won't key: the
# bundle glob `resources/hamlib/*` still matches the tracked licence .txt files,
# so the installer builds green and ships with no Hamlib in it. Every non-Icom
# rig is then CAT-dead while a native CI-V Icom works perfectly — which reads
# exactly like a code regression. Fail here instead.
bad="$(gaps)"
if [ -n "$bad" ]; then
  {
    echo
    echo "Hamlib staging FAILED — NOT usable for a Windows build:"
    echo "$bad"
    echo
    echo "Extracted from : $root/bin"
    echo "Staging into   : $DEST"
    echo "Source archive : $URL"
    echo
    echo "The download and its checksum passed, so the archive's layout has most"
    echo "likely changed. Unpack the zip by hand and check where bin/ now lives,"
    echo "then fix WANT / root in scripts/fetch-hamlib.sh."
  } >&2
  exit 1
fi
echo "Hamlib staged → $DEST"
