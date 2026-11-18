#!/usr/bin/env bash
# check-artifact-set.sh — fail unless a downloaded artifact holds exactly the files expected.
#
#   usage: scripts/check-artifact-set.sh <dir> <file>...
#
# release.yml downloads each artifact by its exact name into a directory of its own, then runs
# this before anything smokes, signs or publishes what came down. The name closes the hole a
# pattern left open (any job in the run can upload an artifact, and the build jobs run
# third-party code); this closes the one inside an artifact: a file nobody expected rides along
# into the release just as well as an artifact nobody expected would have.
#
# <dir> must contain every <file> as a regular file, and nothing else: no extra file, no
# subdirectory, no symlink. Each <file> is a plain name, not a path or a pattern.
set -euo pipefail

[ $# -ge 2 ] || { echo "usage: $0 <dir> <file>..." >&2; exit 2; }
dir=$1; shift
[ -d "$dir" ] || { echo "check-artifact-set: $dir is not a directory (the artifact did not download?)" >&2; exit 1; }

want=$(printf '%s\n' "$@" | LC_ALL=C sort)
got=$(cd "$dir" && find . -mindepth 1 | sed 's|^\./||' | LC_ALL=C sort)
if [ "$got" != "$want" ]; then
  {
    echo "check-artifact-set: $dir does not hold exactly the expected files."
    echo "  missing: $(LC_ALL=C comm -23 <(printf '%s\n' "$want") <(printf '%s\n' "$got") | tr '\n' ' ')"
    echo "  extra:   $(LC_ALL=C comm -13 <(printf '%s\n' "$want") <(printf '%s\n' "$got") | tr '\n' ' ')"
  } >&2
  exit 1
fi
for f in "$@"; do
  [ -f "$dir/$f" ] && [ ! -L "$dir/$f" ] || { echo "check-artifact-set: $dir/$f is not a regular file" >&2; exit 1; }
done
echo "check-artifact-set: $dir holds exactly: $*"
