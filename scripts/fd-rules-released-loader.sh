#!/usr/bin/env bash
# scripts/fd-rules-released-loader.sh <tag> <rules-file> — does Nexus <tag>'s own loader accept
# <rules-file>? Builds `fd-rules-check` as that release shipped it and runs it on the file.
#
# WHY. An installed Nexus validates a downloaded fd-rules.json with ITS OWN loader, and refuses the
# whole file over a single value it cannot read: it keeps the rules it had and stops receiving
# updates. `.github/workflows/fd-rules.yml` publishes ONE file to every install, and its own gate is
# the landing's loader, which knows every new value. So a file only the newest loader accepts can
# cut every older install off from rules updates without any check going red — that nearly shipped
# once, with a header added to a list an older build checks. New rules ride new keys, which older
# loaders ignore; this is what proves they really do.
#
# The release's tree comes from `git archive <tag>` (the tag is fetched first when this checkout
# does not have it: CI's checkout is shallow) and is built in its own target directory, never this
# tree's, so two loaders can never run each other's code.
#
# Exit: 0 the release accepts the file · 1 it refuses it (its own reason is printed above) ·
#       2 usage, or the release could not be fetched or built.
set -u
tag=${1:-}
file=${2:-}
if [ -z "$tag" ] || [ ! -f "$file" ]; then
  echo "usage: $0 <tag> <rules-file>" >&2
  exit 2
fi
root=$(git rev-parse --show-toplevel) || exit 2
file=$(realpath "$file")
work=${FD_RULES_LOADER_DIR:-$root/target/fd-rules-loader}/$tag
if ! git -C "$root" rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
  git -C "$root" fetch -q --no-tags --depth 1 origin "refs/tags/$tag:refs/tags/$tag" || {
    echo "cannot fetch $tag" >&2
    exit 2
  }
fi
mkdir -p "$work/src" || exit 2
git -C "$root" archive "$tag" crates libtempo tests Cargo.toml Cargo.lock | tar -x -C "$work/src" || {
  echo "cannot extract $tag" >&2
  exit 2
}
(cd "$work/src" && CARGO_TARGET_DIR="$work/target" cargo build --quiet --locked -p tempo-core --bin fd-rules-check) || {
  echo "cannot build $tag's fd-rules-check" >&2
  exit 2
}
"$work/target/debug/fd-rules-check" "$file"
rc=$?
if [ "$rc" -ne 0 ]; then
  echo "Nexus $tag refuses $file (exit $rc): every install still on $tag would stop receiving rules updates." >&2
  exit 1
fi
