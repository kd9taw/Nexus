#!/usr/bin/env bash
# Build the Hamlib (rigctld) runtime FROM SOURCE for the host Unix platform and stage it as a
# Tauri bundle resource at src-tauri/resources/hamlib/, so the Linux and macOS bundles ship CAT
# rig control with zero extra installs — the same promise the Windows installer has always kept.
#
# WHY THIS EXISTS (2026-08-24). Windows bundled rigctld from the start; Linux and macOS did not.
# The .deb papered over it with a `depends: libhamlib-utils`, so apt fetched Hamlib and nobody
# noticed the hole. The AppImage has no such mechanism, and an AppImage is chosen precisely
# BECAUSE it installs nothing — so the one package whose whole promise is "download it and run
# it" was the one that could not talk to a radio until the operator found out, unaided, that
# they needed `sudo apt install libhamlib-utils`. It shipped Hamlib's five licence texts and no
# Hamlib. macOS had the same hole with `brew install hamlib`.
#
# WHY FROM SOURCE rather than unpacking a distro package: one pinned version (matching the
# Windows bundle) on every platform, no coupling to whatever Hamlib a given Debian or Homebrew
# happens to carry, and no second answer to "which Hamlib is this?" in a bug report.
#
# LICENSING (checked per-file 2026-08-24, both against upstream headers and Debian's own audit):
# the Hamlib LIBRARY is LGPL-2.1-or-later and the TOOLS are GPL-2.0-or-later — 5 SPDX
# GPL-2.0-or-later + 14 SPDX LGPL-2.1-or-later, and ZERO GPL-2.0-only files, confirmed with a
# positive control. Both are or-later, so both may be used under GPL-3, which is what Nexus is
# (GPL-3.0-only). See the Hamlib entry in NOTICE.
#
#   ./scripts/fetch-hamlib-unix.sh        # idempotent; skips if already staged
set -euo pipefail

VER=4.7.1
TARBALL="hamlib-${VER}.tar.gz"
URL="https://github.com/Hamlib/Hamlib/releases/download/${VER}/${TARBALL}"
SHA256=d197a08a3d5d936d7571ae573f745bbba619e88998742c8267e3fcb0fb3d5974

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DEST="$REPO/src-tauri/resources/hamlib"

case "$(uname -s)" in
  Linux)  HOST=linux; LIBEXT=so ;;
  Darwin) HOST=macos; LIBEXT=dylib ;;
  *) echo "fetch-hamlib-unix.sh is for Linux/macOS; Windows uses fetch-hamlib.sh" >&2; exit 1 ;;
esac

# The four programs Nexus launches. rigctld is CAT, rotctld is the rotator, and rigctl is the
# baud ladder's one-shot prober (`rigctl -l` also backs the model list).
BINS=(rigctld rigctl rotctld rotctl)
LIC=(COPYING COPYING.LIB LICENSE AUTHORS)

# The smallest real Hamlib program is a few tens of KB; this floor cleanly separates a staged
# binary from an empty or truncated one, exactly as in fetch-hamlib.sh.
MIN_BYTES=16384

# Names every wanted file that is absent or implausibly small; silence means staged.
#
# This is BOTH the "already staged?" test and the post-condition after the build, because the
# copy loop cannot report its own failure — and a silent miss here is invisible until an
# operator's rig will not key, since the bundle glob `resources/hamlib/*` still matches the
# tracked licence .txt files and the build goes green with no Hamlib in it. That is the exact
# failure this whole script exists to end; it must not be reintroduced by the script itself.
gaps() {
  local f n
  for f in "${BINS[@]}"; do
    if [ ! -f "$DEST/$f" ]; then
      echo "  $f — MISSING"
    else
      n=$(wc -c <"$DEST/$f")
      [ "$n" -ge "$MIN_BYTES" ] || echo "  $f — only $n bytes (truncated?)"
    fi
  done
  # The shared library the four programs load. Without it they exist and cannot run, which is
  # worse than absent: `runs_ok` rejects them one at a time and CAT dies with no clear cause.
  ls "$DEST"/libhamlib.*"$LIBEXT"* >/dev/null 2>&1 || echo "  libhamlib.$LIBEXT — MISSING"
}

if [ -z "$(gaps)" ]; then
  echo "Hamlib already staged at $DEST"; exit 0
fi

for tool in make cc; do
  command -v "$tool" >/dev/null || { echo "need $tool to build Hamlib" >&2; exit 1; }
done

tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT
echo "Downloading Hamlib $VER source…"
(command -v curl >/dev/null && curl -fsSL -o "$tmp/$TARBALL" "$URL") || wget -qO "$tmp/$TARBALL" "$URL"
if command -v sha256sum >/dev/null; then
  echo "$SHA256  $tmp/$TARBALL" | sha256sum -c - || { echo "checksum mismatch — aborting" >&2; exit 1; }
else
  echo "$SHA256  $tmp/$TARBALL" | shasum -a 256 -c - || { echo "checksum mismatch — aborting" >&2; exit 1; }
fi

tar xzf "$tmp/$TARBALL" -C "$tmp"
src="$tmp/hamlib-${VER}"
pfx="$tmp/prefix"

# THE RPATH IS THE WHOLE TRICK. The staged programs sit beside their libhamlib in the bundle, so
# they must look NEXT TO THEMSELVES for it — not at a configure-time --prefix that will not exist
# on the operator's machine, and not at whatever libhamlib the host happens to have (loading the
# host's would defeat the point and, worse, would work on the developer's box and fail on the
# user's). $ORIGIN / @loader_path is resolved by the loader at run time relative to the binary.
#
# ⚠️ IT CANNOT GO THROUGH LDFLAGS, and two failed attempts are why. libtool OWNS `-rpath` — it
# parses the flag out of the link line and rewrites it for its own relinking scheme. Passing
# `-Wl,-rpath,$ORIGIN` got mangled into a dangling `-Wl,-rpath -Wl,-o`, which ate the output
# argument and failed the link outright (`cannot find libhamlib.so.4`). Escaping it as
# `$$ORIGIN` to survive libtool's second shell only moved the problem: the first attempt wrote
# `RUNPATH=[RIGIN:/tmp/…/prefix/lib]` — `$O` expanded to nothing — and the staged rigctld then
# silently loaded the BUILD MACHINE's system libhamlib, passing every check on a developer box
# with Hamlib installed and dying on an operator's box without it.
#
# So set it AFTER the build, with the tool made for it. patchelf is one apt/brew package and it
# is deterministic; arguing with libtool is neither.

echo "Configuring Hamlib $VER ($HOST)…"
( cd "$src" && ./configure \
    --prefix="$pfx" \
    --disable-static \
    --disable-silent-rules \
    --without-cxx-binding \
    --without-perl-binding \
    --without-python-binding \
    --without-tcl-binding \
    --without-lua-binding \
    --without-indi \
    >"$tmp/configure.log" 2>&1 ) || { echo "configure FAILED — see $tmp/configure.log" >&2; tail -25 "$tmp/configure.log" >&2; exit 1; }

echo "Building…"
( cd "$src" && make -j"$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 2)" >"$tmp/build.log" 2>&1 && make install >>"$tmp/build.log" 2>&1 ) \
  || { echo "build FAILED — see $tmp/build.log" >&2; tail -25 "$tmp/build.log" >&2; exit 1; }

mkdir -p "$DEST"
for f in "${BINS[@]}"; do
  cp "$pfx/bin/$f" "$DEST/" && echo "  + $f"
done
# ONE library, resolved through its symlinks and named for the SONAME — that is the single name
# the loader ever asks for (`readelf -d` → `SONAME: libhamlib.so.4`). Copying the whole
# libhamlib.so / .so.4 / .so.4.0.7 family with `cp -L` instead put THREE 29 MB copies of the same
# file in every bundle.
#
# ⚠️ PLAIN if/else, NOT `soname=$(case … esac)`. macOS ships bash 3.2, which rejects a `case`
# inside a command substitution — "syntax error near unexpected token `;;`". It parses fine on
# the bash 5 this was written on, so the macOS job is the only place it could ever show up, and
# it did: the 1.9.0 release build died here.
if [ "$HOST" = linux ]; then
  soname=$(readelf -d "$pfx/lib/libhamlib.$LIBEXT" 2>/dev/null | sed -n 's/.*SONAME.*\[\(.*\)\].*/\1/p')
else
  soname="libhamlib.4.$LIBEXT"
fi
[ -n "$soname" ] || { echo "could not read libhamlib's SONAME — refusing to guess" >&2; exit 1; }
cp -L "$pfx/lib/$soname" "$DEST/$soname" && echo "  + $soname"

# NOW the rpath: point every staged program (and the library itself) at its own directory.
case "$HOST" in
  linux)
    command -v patchelf >/dev/null || { echo "need patchelf (apt install patchelf)" >&2; exit 1; }
    for f in "${BINS[@]}"; do patchelf --set-rpath '$ORIGIN' "$DEST/$f"; done
    patchelf --set-rpath '$ORIGIN' "$DEST/$soname"
    ;;
  macos)
    command -v install_name_tool >/dev/null || { echo "need install_name_tool (Xcode CLT)" >&2; exit 1; }
    # The tools record libhamlib by its install-name; rewrite that to @rpath and add @loader_path.
    for f in "${BINS[@]}"; do
      old=$(otool -L "$DEST/$f" | awk '/libhamlib/ {print $1; exit}')
      [ -n "${old:-}" ] && install_name_tool -change "$old" "@rpath/$soname" "$DEST/$f"
      install_name_tool -add_rpath "@loader_path" "$DEST/$f" 2>/dev/null || true
    done
    install_name_tool -id "@rpath/$soname" "$DEST/$soname"
    ;;
esac

# libusb comes with us: a USB-attached rig is the one case where its absence is not a theory,
# and it is 100 KB. Everything else libhamlib wants (libc, libstdc++, libudev, libz, libcap) is
# on any desktop that can run the app at all.
#
# ⚠️ `ldd` IS LINUX-ONLY. macOS has no such command, and under `set -e` the failed command
# substitution took the whole script down with exit 127 — after Hamlib had already built, so
# the log looked like a successful build that stopped for no reason. macOS reads the same
# information from `otool -L`.
if [ "$HOST" = linux ]; then
  dep=libusb-1.0.so.0
  hostlib=$(ldd "$DEST/rigctld" 2>/dev/null | awk -v d="$dep" '$1==d {print $3}' || true)
else
  dep=libusb-1.0.0.dylib
  # Probe the TOOLS AND THE LIBRARY: otool -L lists direct dependencies only (unlike ldd,
  # which is transitive), and whether libusb is linked by the tools or only by libhamlib
  # itself varies by Hamlib version — 4.7.1's tools carry it, the Linux build's don't.
  hostlib=$({ otool -L "$DEST/rigctld"; otool -L "$DEST/$soname"; } 2>/dev/null \
            | awk '$1 !~ /^@/ && $1 ~ /libusb/ {print $1; exit}' || true)
fi
if [ -n "${hostlib:-}" ] && [ -f "$hostlib" ]; then
  cp -L "$hostlib" "$DEST/$dep"
  echo "  + $dep (from host)"
  # ⚠️ AND ON macOS, REPOINT IT. The copy is not enough: rigctld still records the absolute
  # HOMEBREW path it was linked against (/opt/homebrew/opt/libusb/...), which does not exist on
  # an operator's Mac. Without this rewrite the bundled libusb sits there unused and a
  # USB-attached rig fails on exactly the machines that never had brew — i.e. the ones this
  # whole change exists to serve.
  if [ "$HOST" = macos ]; then
    # ⚠️ "${BINS[@]}" AND "$soname" — THE LIBRARY CARRIES THE REFERENCE TOO, and it is the
    # one that shipped broken: 1.10.0 repointed the four tools and left libhamlib.4.dylib
    # still naming /opt/homebrew/opt/libusb/... — resolvable on every CI runner (brew is
    # right there, so the release gate's `rigctld --version` stayed green) and absent on an
    # operator's Mac without Homebrew's libusb, where dyld killed rigctld AND every one-shot
    # rigctl baud-ladder rung before main. That is the mac 1.10.0 "rig never answered at any
    # speed" CAT regression. The recorded path is read per file: it is a per-file load
    # command, not a global.
    for f in "${BINS[@]}" "$soname"; do
      old=$(otool -L "$DEST/$f" 2>/dev/null | awk '$1 !~ /^@/ && $1 ~ /libusb/ {print $1; exit}' || true)
      [ -n "${old:-}" ] && install_name_tool -change "$old" "@rpath/$dep" "$DEST/$f" 2>/dev/null || true
    done
    install_name_tool -id "@rpath/$dep" "$DEST/$dep" 2>/dev/null || true
  fi
fi

# Strip: an unstripped libhamlib is 29 MB against the Windows DLL's 12.
case "$HOST" in
  linux) strip --strip-unneeded "$DEST/$soname" "${BINS[@]/#/$DEST/}" 2>/dev/null || true ;;
  macos) strip -x "$DEST/$soname" "${BINS[@]/#/$DEST/}" 2>/dev/null || true ;;
esac
# The licence texts are TRACKED (they came from the Windows zip, CRLF), and Hamlib's LGPL
# requires we ship them. Write one only if it is ABSENT — copying unconditionally rewrote all
# four with LF line endings, showing up as 1267 changed lines of identical text and tripping
# build-linux.sh's own "modified a tracked licence file" guard. Same text, no reason to touch it.
for f in "${LIC[@]}"; do
  [ -f "$src/$f" ] && [ ! -f "$DEST/${f}.txt" ] && cp "$src/$f" "$DEST/${f}.txt" || true
done

# THE CAT-DEAD GUARD — see gaps() above. Everything between here and the download can fail
# without failing the script; fail loudly instead of shipping a licence-only hamlib directory.
bad="$(gaps)"
if [ -n "$bad" ]; then
  {
    echo
    echo "Hamlib staging FAILED — NOT usable for a $HOST build:"
    echo "$bad"
    echo
    echo "Built from   : $src"
    echo "Install pfx  : $pfx"
    echo "Staging into : $DEST"
    echo "Source       : $URL"
    echo
    echo "configure.log and build.log are under $tmp (removed on exit — re-run with"
    echo "'trap - EXIT' commented in to keep them)."
  } >&2
  exit 1
fi

# PROVE THEY RUN, here, rather than discovering it on an operator's radio. A binary whose
# libhamlib cannot be resolved is executable, present, the right size, and completely dead —
# the exact class `runs_ok` exists to reject at run time. Catch it at build time instead.
#
# ⚠️ AND IT MUST LOAD *OUR* LIBRARY. `--version` succeeding proves nothing on a machine that has
# Hamlib installed — the first version of this check passed while rigctld was resolving
# /lib/x86_64-linux-gnu/libhamlib.so.4 and the bundle's own copy went untouched. Assert the
# resolved path is INSIDE the staging directory, which is the thing the operator's machine will
# actually depend on.
if [ "$HOST" = linux ]; then
  resolved=$(ldd "$DEST/rigctld" 2>/dev/null | awk '/libhamlib/ {print $3}')
  case "${resolved:-}" in
    "$DEST"/*) : ;;
    *)
      echo "Staged rigctld resolves libhamlib to '${resolved:-<nothing>}', NOT the copy beside it." >&2
      echo "The rpath did not take. Check RUNPATH:" >&2
      readelf -d "$DEST/rigctld" | grep -E 'RPATH|RUNPATH' >&2
      exit 1
      ;;
  esac
else
  # The macOS equivalent, and it catches a class `--version` never can. On the RUNNER a
  # Homebrew path baked into the binary resolves perfectly — brew is right there. On the
  # operator's Mac it does not exist, which is the entire population this change is for. So
  # assert every dependency is either OURS (@rpath) or a genuine system library —
  #
  # — FOR EVERY STAGED MACH-O, NOT JUST rigctld. The first version of this check read
  # rigctld alone and passed while libhamlib.4.dylib — the library rigctld loads — still
  # named /opt/homebrew/opt/libusb/lib/libusb-1.0.0.dylib: it verified the already-repointed
  # half against itself, and 1.10.0 shipped mac CAT that dyld killed before main on any Mac
  # without Homebrew's libusb (a bad path ANYWHERE in the load chain kills the tool). Each
  # file also carries a positive control: a real Mach-O always links libSystem, so a scan
  # that cannot see it is reading nothing and its clean verdict would be the broken-check lie.
  for f in "${BINS[@]/#/$DEST/}" "$DEST/$soname" "$DEST"/libusb-*.dylib; do
    [ -f "$f" ] || continue
    deps=$(otool -L "$f" 2>/dev/null || true)
    printf '%s\n' "$deps" | grep -q '/usr/lib/libSystem.B.dylib' || {
      echo "otool read no dependencies from $f — cannot verify the bundle" >&2; exit 1; }
    bad=$(printf '%s\n' "$deps" | tail -n +2 | awk '{print $1}' \
          | grep -vE '^@rpath/|^@loader_path/|^/usr/lib/|^/System/' || true)
    if [ -n "$bad" ]; then
      {
        echo "Staged $(basename "$f") depends on paths that will not exist on an operator's Mac:"
        echo "$bad" | sed 's/^/  /'
        echo "Every dependency must be @rpath/… (bundled beside it) or a system library."
      } >&2
      exit 1
    fi
  done
fi
if ! "$DEST/rigctld" --version >/dev/null 2>&1; then
  {
    echo
    echo "Staged rigctld cannot RUN — almost certainly the rpath did not take, so it cannot"
    echo "find the libhamlib staged beside it. Diagnose with:"
    case "$HOST" in
      linux) echo "  ldd $DEST/rigctld" ;;
      macos) echo "  otool -L $DEST/rigctld" ;;
    esac
    "$DEST/rigctld" --version 2>&1 | head -5 | sed 's/^/  /'
  } >&2
  exit 1
fi

echo "Hamlib $VER staged → $DEST ($("$DEST/rigctld" --version 2>&1 | head -1))"
