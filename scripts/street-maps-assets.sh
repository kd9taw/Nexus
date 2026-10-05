#!/usr/bin/env bash
# Build the street map's fonts-and-icons archive: the `assets` that streetmaps.json names and
# crates/street-map/src/assets.rs checks and installs. .github/workflows/street-maps.yml runs this
# in its plan job, which holds no secret, because the sprite generator below is compiled from
# third-party sources.
#
#   OFL.txt                                 the fonts' licence (SIL OFL 1.1)
#   ICONS-LICENSE.txt                       the icons' licence (MIT, tangrams/icons)
#   fonts/<stack>/<range>.pbf               Noto Sans Regular, Medium and Italic glyphs
#   sprites/{light,dark}{,@2x}.{json,png}   the icon sheets
#
# The sheets are generated, not downloaded: the published ones (basemaps-assets sprites/v4) lack
# the `townhall` icon the style names. They come from upstream's generator at the style's own
# commit, with the icons the style names and the generator leaves out added (SPRITE_ADDITIONS in
# scripts/street-maps.mjs, which also checks the result). Every source is a commit that git checks
# each object against, and the archive is reproducible byte for byte, so an unchanged set keeps
# its SHA-256 and its key, and installs do not fetch it again.
#
# Usage: scripts/street-maps-assets.sh <work-dir> <out-dir>
#   <work-dir> must not exist yet; <out-dir>/assets.tar.gz is written.
set -euo pipefail

# @protomaps/basemaps 5.7.2's gitHead (its npm record): the style the app draws, and its generator.
BASEMAPS_COMMIT=3ea8293a28131c3dc63f1bb20827bdb8a76df06f
# protomaps/basemaps-assets, 2025-10-31: the Noto Sans glyph ranges and their OFL.txt.
ASSETS_COMMIT=028c18f713baecad011301ff7a69acc39bcc2ae7
# tangrams/icons, 2018-11-19: the MIT licence the icons are drawn under.
ICONS_COMMIT=92510779634f4a006c61ea70e50cb8c52c765a81

work=${1:?usage: street-maps-assets.sh <work-dir> <out-dir>}
out=${2:?usage: street-maps-assets.sh <work-dir> <out-dir>}
here=$(cd "$(dirname "$0")" && pwd)
if [ -e "$work" ]; then
  echo "street-maps-assets: $work already exists" >&2
  exit 1
fi
mkdir -p "$work" "$out"
work=$(cd "$work" && pwd)

# <owner/repo> <commit> <dir> [<folder>...]: the repository's top-level files and the named
# folders, exactly as they are at <commit>.
fetch_at() {
  local repo=$1 commit=$2 dir=$3
  shift 3
  git init -q "$dir"
  git -C "$dir" remote add origin "https://github.com/$repo"
  git -C "$dir" sparse-checkout set "$@"
  git -C "$dir" fetch -q --depth 1 --filter=blob:none origin "$commit"
  git -C "$dir" checkout -q FETCH_HEAD
  test "$(git -C "$dir" rev-parse HEAD)" = "$commit"
}
fetch_at protomaps/basemaps "$BASEMAPS_COMMIT" "$work/basemaps" sprites
fetch_at protomaps/basemaps-assets "$ASSETS_COMMIT" "$work/basemaps-assets" \
  "fonts/Noto Sans Regular" "fonts/Noto Sans Medium" "fonts/Noto Sans Italic"
fetch_at tangrams/icons "$ICONS_COMMIT" "$work/icons"

stage=$work/stage
mkdir -p "$stage/fonts" "$stage/sprites"
cp "$work/basemaps-assets/fonts/OFL.txt" "$stage/OFL.txt"
cp "$work/icons/LICENSE.md" "$stage/ICONS-LICENSE.txt"
for stack in "Noto Sans Regular" "Noto Sans Medium" "Noto Sans Italic"; do
  cp -R "$work/basemaps-assets/fonts/$stack" "$stage/fonts/"
done

# The generator, with the project's pinned compiler whatever the box's default is.
export RUSTUP_TOOLCHAIN=${RUSTUP_TOOLCHAIN:-1.93.1}
export CARGO_TARGET_DIR=$work/target
(cd "$work/basemaps/sprites" && cargo build --release --locked --quiet)
for look in light dark; do
  node "$here/street-maps.mjs" flavor "$work/basemaps/sprites/flavors/$look.json" > "$work/$look.json"
  (cd "$work/basemaps/sprites" && "$CARGO_TARGET_DIR/release/spritegen" refill.svg "$work/$look.json" "$stage/sprites/$look")
done

node "$here/street-maps.mjs" check-assets "$stage"
tar --format=ustar --sort=name --mtime=@0 --owner=0 --group=0 --numeric-owner --mode=u=rwX,go=rX \
  -C "$stage" -cf - OFL.txt ICONS-LICENSE.txt fonts sprites | gzip -n -9 > "$out/assets.tar.gz"
sha256sum "$out/assets.tar.gz"
