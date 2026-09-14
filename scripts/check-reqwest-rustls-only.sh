#!/usr/bin/env bash
# scripts/check-reqwest-rustls-only.sh — fail if reqwest can reach a native-tls backend.
#
# WHY. Cargo unifies features across the whole dependency graph, so reqwest's `native-tls`
# feature is not a per-client switch. Once ANY crate turns it on (or, in reqwest 0.12,
# `default-tls`, which is native-tls there), every `reqwest::Client::builder()` in the app
# silently moves off rustls + the bundled webpki roots onto the operating system's TLS stack and
# its weaker CBC suites. Measured 2026-09-13: with the feature compiled in, a plain builder
# handshook with HRDLog.net, which rustls cannot.
#
# HRDLog's upload genuinely needs that stack, so crates/propagation/src/live/hrdlog.rs uses the
# `native-tls` crate DIRECTLY, locked to that one host. `native-tls` appearing in a lockfile is
# therefore expected. This check is what keeps the other ~38 reqwest clients (QRZ, ClubLog, LoTW,
# eQSL, the Remote transport, ...) from quietly joining it.
#
# Two independent reads; either one fails the check:
#   1. the lockfile — a `reqwest` entry whose dependency list names native-tls, hyper-tls or
#      tokio-native-tls (any feature that selects native-tls pulls those in);
#   2. `cargo tree` over all features and all targets — the same three crates as direct
#      dependencies of a resolved reqwest, or an enabled reqwest feature named after native-tls.
#
# Usage: scripts/check-reqwest-rustls-only.sh [path/to/Cargo.toml ...]
#   With no arguments: the workspace and src-tauri manifests (each has its own Cargo.lock).
# Exit: 0 clean · 1 reqwest has a native-tls backend · 2 the check could not run.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
if [ "$#" -gt 0 ]; then
  manifests=("$@")
else
  manifests=("$REPO/Cargo.toml" "$REPO/src-tauri/Cargo.toml")
fi

NATIVE='native-tls|hyper-tls|tokio-native-tls'
bad=0
for manifest in "${manifests[@]}"; do
  lock="$(dirname "$manifest")/Cargo.lock"
  [ -s "$manifest" ] && [ -s "$lock" ] || { echo "no manifest/lockfile at $manifest"; exit 2; }

  # 1 — the lockfile. Prints "VERSION <v>" per reqwest entry and "HIT <v> <dep>" per offender.
  lockread=$(awk -v native="^($NATIVE)\$" '
    /^\[\[package\]\]/ { name = ""; ver = ""; deps = 0; next }
    /^name = /         { name = $3; gsub(/"/, "", name) }
    /^version = /      { ver = $3; gsub(/"/, "", ver); if (name == "reqwest") print "VERSION " ver }
    /^dependencies = \[/ { deps = 1; next }
    deps && /^\]/      { deps = 0; next }
    deps && name == "reqwest" { d = $1; gsub(/[",]/, "", d); if (d ~ native) print "HIT " ver " " d }
  ' "$lock")
  versions=$(printf '%s\n' "$lockread" | awk '$1 == "VERSION" { print $2 }')
  # No reqwest at all means this check is pointed at the wrong thing, not that all is well.
  [ -n "$versions" ] || { echo "$lock: no reqwest entry found — the check is broken"; exit 2; }
  while read -r _ v dep; do
    [ -n "${v:-}" ] || continue
    echo "FAIL $lock: reqwest $v depends on $dep"
    bad=1
  done < <(printf '%s\n' "$lockread" | awk '$1 == "HIT"')

  # 2 — cargo tree. Inverted queries only: cargo refuses `--all-features` with `-p` on a package
  # outside the workspace, and reqwest always is one.
  common=(--manifest-path "$manifest" --locked --all-features --target all --prefix none)
  # 2a — who depends directly on each native-tls backend crate? reqwest must not be among them.
  for crate in native-tls hyper-tls tokio-native-tls; do
    if out=$(cargo tree "${common[@]}" -e normal --depth 1 -i "$crate" 2>&1); then
      if hits=$(printf '%s\n' "$out" | grep -E '^reqwest v'); then
        echo "FAIL $manifest: $crate is a direct dependency of:"; echo "$hits"; bad=1
      fi
    elif printf '%s\n' "$out" | grep -q 'did not match any packages'; then
      : # not in this graph at all
    else
      echo "cargo tree -i $crate failed for $manifest:"; echo "$out"; exit 2
    fi
  done
  # 2b — the features each resolved reqwest is built with.
  for v in $versions; do
    if ! feats=$(cargo tree "${common[@]}" -e features -i "reqwest@$v" 2>&1); then
      echo "cargo tree -e features failed for reqwest@$v in $manifest:"; echo "$feats"; exit 2
    fi
    # Positive control on the output itself: an empty or unrelated tree must not read as clean.
    printf '%s\n' "$feats" | grep -q "^reqwest feature " \
      || { echo "cargo tree listed no reqwest $v features for $manifest — the check is broken"; exit 2; }
    if hits=$(printf '%s\n' "$feats" | grep -E '^reqwest feature "(__)?native-tls'); then
      echo "FAIL $manifest: reqwest $v has native-tls features enabled:"; echo "$hits"; bad=1
    fi
    echo "checked $manifest: reqwest $v"
  done
done

if [ "$bad" -ne 0 ]; then
  echo
  echo "reqwest has a native-tls backend. Every reqwest client in the app now runs on the OS TLS"
  echo "stack instead of rustls. Find the crate that enabled it with:"
  echo "  cargo tree --manifest-path <manifest> -e features -i reqwest@<version>"
  echo "If one host really needs the OS stack, use the native-tls crate directly, as"
  echo "crates/propagation/src/live/hrdlog.rs does — not the reqwest feature."
  exit 1
fi
echo "reqwest is rustls-only in every checked lockfile"
