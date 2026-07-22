# Multi-radio: what happens if we have to back out

Written 2026-07-22, after the operator asked the right question before the expensive part
starts. Short answer: **for everything built so far, backing out is not required — stopping is
enough.** That stops being true at one specific, identifiable line, and this document names it.

## Why nothing so far needs unwinding

Every step to date is **inert at runtime**, verified rather than asserted:

| Claim | How it is verified |
|---|---|
| `DecodeJob.ctx` is always `None` in production | all 3 construction sites hard-code `None`; `with_ctx` is called only from tests |
| The `ctx: None` decode path is byte-for-byte the shipped one | the match arm is literally `None => decode()` |
| `StationCore` is a pure relocation | owned **by value** on `Engine`, no `Arc`/`Mutex`, chain count effectively one |
| The vendored Fortran hoists change no logic | `watch_identity` byte-identical + ft8/ft4/tempo-fast decode parity green |
| Station-wide behaviour is unchanged | `station_identity` byte-identical (built specifically because `watch_identity` is blind to it) |
| The chain **addressing** layer is unreachable | `Chains` is not `.manage()`d, so no command can obtain one; `Chains::add` always errors; `chain_of` has no production caller; `r<id>` and `w<n>` windows are both refused by `open_panel_window` |
| Every window openable today is byte-identical | same label, same URL, same `bandmap-window-<slug>.json` filename — `panel_key` maps a legacy `panel-bandmapCw` to `("bandmapCw", Main)` |

Anchor tag: **`multiradio-foundation-inert`**.

So if the program is abandoned today, the cost is:
- some unused machinery in the tree (a `ctx` field that is always `None`, a `StationCore`
  split that is arguably good hygiene anyway);
- the **vendored-edit re-apply burden**: `packjt77.f90`, `ft4_decode.f90`,
  `tempofast_decode.f90` and the three `*_downsample.f90` files carry `MODIFIED FOR NEXUS`
  hoists that a future WSJT-X refresh must re-apply. This is the only durable cost, and it is
  greppable by design.
- **nothing at runtime.**

## The line where that stops being true

**Lifting the one-chain cap.** Up to and including "chain skeleton capped at one", every
commit is behaviour-neutral and ships safely inside a normal release. The moment a second
chain can spawn, real behaviour changes: two decoders, two rigs, contended locks, and TX
arbitration.

**Status: the cap now exists and holds.** The `Chains` registry was built as part of the
addressing step (`src-tauri/src/chains.rs`), capped at `MAX_CHAINS = 1`, and `Chains::add`
errors unconditionally. It holds harder than that in practice: the registry is deliberately
**not** registered as Tauri managed state, so no command can obtain one at all.

That last point is a correction worth keeping. The registry was briefly `.manage()`d keyed by a
boot snapshot of `settings.active_radio` — but switching radios in Settings does *not* rebuild
the engine (`Engine::set_active_radio` mutates in place), so the entry would have sat filed
under a dead profile id with no refresh hook. Storing a knowably-wrong radio→engine association
is precisely the wrong-rig class this layer exists to prevent, one level below where the token
grammar guards it. Nothing read the registry, so managing it bought nothing. Re-keying on
radio-switch is the **cap-lift's** first problem, and the cap-lift is where the registry gets
its first reader.

## What we are doing differently past that line

1. **The cap-lift and everything after goes on a branch** (`multiradio-live`), not `main`.
   Rationale for keeping the foundation on `main` but not this: the foundation is inert and
   benefits from being exercised by every real build, whereas the cap-lift is the first change
   that can regress a single-radio operator. A long-lived branch is bad for a 12k-line
   `engine.rs` refactor (merge pain), but correct for the smaller, riskier tail.

2. **A runtime kill switch ships with the cap-lift.** Not a rebuild, not a downgrade: a
   setting that forces one chain. If two radios misbehave on the air, the operator turns it
   off and keeps operating. Rollback that requires reinstalling an older build during a
   contest is not rollback.

3. **The one-chain path stays the default** until the operator opts in, so the population of
   users exposed to a regression is exactly "people who deliberately enabled it".

4. **Both golden fixtures stay the gate.** `watch_identity` covers per-chain state,
   `station_identity` covers station-wide. Neither may be rebaselined to make a failure go
   away — a fixture change is a bug report.

## If we do decide to unwind

The multi-radio commits between `v0.14.0` and `multiradio-foundation-inert` are individually
revertable but **interleaved with unrelated release work** (the TempoFast rename, DXKeeper,
the LoTW and POTA fixes). A blanket range revert would take those with it. Revert set, newest
first — later ones depend on earlier ones, so revert in this order:

```
(pending)  Phase 1b step 2: chain addressing (chains.rs + lib.rs geometry re-key)
50b4c177  Phase 1b step 1: StationCore
c2f9d229  Phase 1a COMPLETE: AP masks
4fe8d425  CORRECTION: decode guard / TX gap        (comment-only, can keep)
0588a0eb  CORRECTION: context coverage             (comment-only, can keep)
0f9e47f5  Fix context initializer + test hole
14021251  Phase 1a: per-chain contexts
c7a75842  Phase 1a option A: hoist spectrum buffers
3e5faf18  manifest gaps closed, gate hard
165a435a  build gate
c374864c  manifest authoritative
```

The manifest and build gate (`c374864c`, `165a435a`, `3e5faf18`) are worth **keeping even if
multi-radio dies**: they document 615 modem state symbols and fail the build when a vendor
refresh introduces unclassified state. That is useful on its own.

So is the **src-tauri CI gate** added alongside the addressing step. `src-tauri` declares an
empty `[workspace]` table, so it is its own workspace root and `cargo test --workspace` /
`cargo clippy --workspace -D warnings` never reach it — verified by planting a type error there
and watching both exit clean. The Windows job only runs `cargo build --release`, which never
compiles `#[cfg(test)] mod tests`. The entire Tauri command layer therefore had **no automated
gate at all**; that is now fixed and is independent of multi-radio.

Two caveats on that gate, so nobody reads more into it than is there:
- The clippy step is **not** `-D warnings`. `src-tauri` carries **15 pre-existing lints**
  (MSRV, type complexity, redundant casts) — a direct consequence of never having been linted.
  Denying crate-wide would fail on day one, and burning it down is unrelated churn in a
  10k-line `lib.rs`. **Backlog item**, tracked here.
- `chains.rs` therefore carries `#![deny(warnings, clippy::all)]` itself, so the addressing code
  *is* held to the standard while the backlog stays visible. Verified by planting a dead
  function in it: `cargo clippy` exits 101.

## The honest risk that is not covered by any of this

Complexity in `engine.rs` is cumulative and does not revert cleanly once later work builds on
it. The mitigation is the branch boundary above, and the discipline of keeping each step
behaviour-neutral so it can be judged on its own. If a step cannot be made behaviour-neutral,
that is the signal to stop and reconsider — not to push through and rely on the revert list.
