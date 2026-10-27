# Vendored WSJT-X–derived modem source (`libtempo`)

This directory is the **complete corresponding source** (GPL-3.0-only, §6) for
the FT8 / FT4 / FT1 modem that `libtempo` compiles into the Nexus binary. It is
vendored in-tree so that a fresh clone builds the modem with **no external
checkout** — `libtempo/CMakeLists.txt` points `WX` here by default.

## Provenance and license

The DSP sources here are derived from **WSJT-X**, © Joe Taylor (K1JT) and the
WSJT Development Group (Steve Franke K9AN, Bill Somerville G4WJS, Nico Palermo
IV3NWV, and others), licensed under the **GNU GPL, version 3** (GPL-3.0-only; the vendored `lib/` files carry no per-file license headers, so no "or later" grant applies).

- Upstream repository: <https://github.com/WSJTX/wsjtx> (older releases:
  <https://sourceforge.net/projects/wsjt/>)
- Full license text: `COPYING` at the repository root.

This is a **subset** of WSJT-X's `lib/` — the Fortran/C/C++ DSP sources that `libtempo`
builds — and, under `tests/`, some of upstream's own tests for them. Measured against this
tree at the JTTY commit: `lib/` holds **219 source files** (189 `.f90`, 28 `.c`, 2 `.cpp`)
plus 22 headers. 197 of the 219 are named one by one in `libtempo/CMakeLists.txt` — the list
is enumerated, never globbed. Of the other 22, 18 are the parameter/generator/parity/table
files those sources pull in with `include` or `#include`; `libtempo/CMakeLists.txt` names
three as deliberately not compiled (the QRA64 code tables `qra/qracodes/qra12_63_64_irr_b.c`
and `qra/qracodes/qra13_64_64_irr_e.c`, and the noise generator `qra/q65/normrnd.c`); and
`ft8/ft8_testmsg.f90` is used by nothing. `tests/` holds 19 of upstream's Fortran unit tests
for the JTTY files and the message list one of them reads; they are not compiled into
`libtempo`.

**None of WSJT-X's Qt/GUI code is included, and that claim is about THIS DIRECTORY.**
Measured: zero occurrences of `#include <Q…>`, `QString`, `QRegularExpression`, `QObject`,
`Q_OBJECT` or `QCoreApplication` across all 261 files, and zero `.ui`, `.qrc`, `.pro`,
`.qml` or `moc_*` files. The only two `.cpp` files are `lib/crc13.cpp` and `lib/crc14.cpp`
(Boost-CRC message checks, no Qt).

⚠️ **The directory scope is the trap, and it has bitten once.** On 2026-08-06 four fragments
of upstream Qt source — `MainWindow::stdCall`'s regex, both `Radio.cpp` patterns, and the
body of `Radio::is_77bit_nonstandard_callsign` — reached the repository through a *test
fixture* under `crates/tempo-core/tests/fixtures/`, nowhere near this directory. The sentence
above stayed literally true the whole time and said nothing about the leak. So: **this README
governs `libtempo/vendor/wsjtx/` only.** Upstream expression anywhere else in the tree is
guarded by `crates/tempo-core/tests/wsjtx_predicate_differential.rs`, which fails the suite if
a `pattern` text field reappears in that fixture ("upstream pattern TEXT is back in the
fixture — that is the licence leak"). Upstream is now pinned by sha256 fingerprint instead of
by reproducing what it says. If you add a WSJT-X-derived measurement anywhere, record the
**fingerprint**, not the text.

One deliberate exception lives outside this directory: `crates/modes/src/wsjt77/`, a Rust
translation of the FT8/FT4 transmit encoder from the copies here (`lib/77bit/packjt77.f90`,
`lib/ft8/genft8.f90`, `lib/ft4/genft4.f90`, `lib/ft8/encode174_91.f90`, `lib/crc14.cpp`,
`lib/ft8/gen_ft8wave.f90`, `lib/ft4/gen_ft4wave.f90`, `lib/ft2/gfsk_pulse.f90`). It is WSJT-X
expression under GPL-3.0-only like everything here; its files carry their own origin and
licence headers, and NOTICE describes it.

**`lib/ft2/` is the one directory here whose upstream is NOT WSJT-X itself** (added
2026-08-16): its eleven mode files are vendored from **Decodium 3** (IU8LMC's GPL-3 fork of
WSJT-X 3.0.0-rc1, <https://github.com/iu8lmc/decodium3-build>) — Decodium's FT2, the variant
with an on-air population, not upstream's abandoned experiment of the same name. Provenance
per file is in `NOTICE` §"Decodium FT2": five are K1JT's FT4 files mechanically renamed, one
is K1JT's `sync4d` with a single IU8LMC change, and the decoder chain
(`ft2_triggered_decode`, `decode174_91_ft2`, `get_ft2_bitmetrics`, `ft2_channel_est`,
`ft2_params`) is IU8LMC's. `gfsk_pulse.f90` in the same directory predates this and is
upstream's. Same license terms as the rest of the tree (GPL-3.0-only, no per-file headers).

## JTTY — vendored from WSJT-X

`lib/jtty/`, `lib/77bit/packjt77_grammar.f90`, `tests/unit/jtty/` and `tests/fixtures/jtty/`
hold WSJT-X's JTTY modem and upstream's own tests for it, taken from
<https://github.com/WSJTX/wsjtx> at the tag `v3.3.0-beta4`, commit
`0f7d5928449ec747c3d1f6da1306f8c94404c3a9` (2026-10-21). Every file is upstream's file at that
commit, byte for byte: Nexus adds no header and changes nothing, so none of them carries a
`MODIFIED FOR NEXUS` note. `crates/tempo-fast-sys/tests/jtty_provenance.rs` holds each file to
its git blob id at that commit, holds the two lists below to its table, and fails if anything
else appears in those directories.

**`v3.3.0-beta4` is a WSJT-X pre-release.** JTTY first appeared in WSJT-X 3.2.0-rc1
(2026-09-23), and no WSJT-X general release carries it yet. When one does, the files are taken
again from that release (see *Re-taking the files* below).

Licence: like the rest of WSJT-X's `lib/`, these files carry no per-file copyright or licence
header. WSJT-X licenses its source under the GNU GPL, version 3 (its `COPYING`, its README, and
its `DOCS-LICENSE.md`: "the source code, which remains licensed under the GNU General Public
License v3.0"), with no "or any later version" grant, so they are GPL-3.0-only here. Authors,
by upstream's commit history: Joe Taylor K1JT, Steve Franke K9AN and David Christle KD0BTO,
with a contribution from Brian Moran N9ADG. `packjt77_grammar.f90` is David Christle's, created
on 2026-07-09 when upstream restructured its 77-bit packer.

### Vendored

The modem, compiled into `libtempo` (20 files, 7,261 lines):

| File | Lines | What it is |
|---|---:|---|
| `lib/jtty/jtty_source_codec.f90` | 474 | the 34-bit atom grammar: call actions, structured exchanges, text |
| `lib/jtty/jtty_mod.f90` | 454 | message normalisation and packing a message into frames |
| `lib/jtty/genjtty.f90` | 262 | message to channel tones; the entries upstream's transmitter calls |
| `lib/77bit/packjt77_grammar.f90` | 1629 | the 77-bit grammar tables, including the ARRL and RAC section list |
| `lib/jtty/tbcc.f90` | 422 | the tail-biting convolutional code: encoding, CRC-12, decoding |
| `lib/jtty/jtty_fec_mod.f90` | 19 | the sync sequence and the code the mode uses |
| `lib/jtty/jtty_tbcc_code_profile.f90` | 106 | the convolutional code's description |
| `lib/jtty/jtty_tbcc_decoder.f90` | 151 | the convolutional decoder |
| `lib/jtty/jtty_tbcc_list_decoder.f90` | 1168 | the list decoder |
| `lib/jtty/gen_jttywave.f90` | 110 | the 4-GFSK waveform |
| `lib/jtty/gen_syncwave.f90` | 26 | the sync waveform |
| `lib/jtty/jtty_mdecode.f90` | 1302 | the multi-signal decoder and message assembly |
| `lib/jtty/jtty_receive_context.f90` | 368 | the streaming receive interface (`jtty_rx_*`) |
| `lib/jtty/subtract_jtty.f90` | 99 | subtracts a decoded signal |
| `lib/jtty/jtty_peakup.f90` | 146 | fine time and frequency |
| `lib/jtty/jtty_payload_correlators.f90` | 108 | payload correlation |
| `lib/jtty/jtty_block_pow.f90` | 101 | coherent block detection |
| `lib/jtty/ana64a.f90` | 19 | the analytic signal |
| `lib/jtty/rjtty_sub.f90` | 105 | the whole-recording decoder upstream's tests drive |
| `lib/jtty/jtty_decode.f90` | 192 | the single-signal decoder |

Upstream's tests, not compiled into `libtempo` (20 files, 3,542 lines): the Fortran unit tests
`tests/unit/jtty/test_jtty_adjacent_decode.f90`, `tests/unit/jtty/test_jtty_block_pow.f90`,
`tests/unit/jtty/test_jtty_continuous_decode.f90`,
`tests/unit/jtty/test_jtty_frequency_window.f90`, `tests/unit/jtty/test_jtty_gap_merge.f90`,
`tests/unit/jtty/test_jtty_get_updates.f90`, `tests/unit/jtty/test_jtty_message_matching.f90`,
`tests/unit/jtty/test_jtty_overlap_decode.f90`, `tests/unit/jtty/test_jtty_pack.f90`,
`tests/unit/jtty/test_jtty_payload_correlators.f90`,
`tests/unit/jtty/test_jtty_receive_state.f90`, `tests/unit/jtty/test_jtty_source_codec.f90`,
`tests/unit/jtty/test_jtty_sticky_retry.f90`, `tests/unit/jtty/test_jtty_surface_ranges.f90`,
`tests/unit/jtty/test_jtty_tbcc_code_profile.f90`,
`tests/unit/jtty/test_jtty_tbcc_decoder.f90`,
`tests/unit/jtty/test_jtty_tbcc_list_decoder.f90`,
`tests/unit/jtty/test_jtty_windowed_decode.f90` and `tests/unit/jtty/test_tbcc.f90`, and
`tests/fixtures/jtty/jtty_msgs.txt`, the message list `test_jtty_pack.f90` reads.

### Not vendored

- The standalone console program: `lib/jtty/jtty.f90`, `lib/jtty/jttycom.f90`,
  `lib/jtty/transmit.f90`, `lib/jtty/update.f90`, `lib/jtty/c_funcs.f90`,
  `lib/jtty/cdatetime.f90`, `lib/jtty/jttyaudio.c` (PortAudio and `conio.h`),
  `lib/jtty/jtty.ini`, `lib/jtty/decodes.txt`, `lib/jtty/jtty_msgs.txt` and
  `lib/jtty/JTTY Messages.mc` (an N1MM macro file).
- `lib/jtty/portaudio.h`: PortAudio's header, © 1999-2002 Ross Bencina and Phil Burk under
  PortAudio's own licence, the one file in upstream's JTTY directory that is not under
  WSJT-X's.
  Only the program above uses it.
- The simulation and laboratory programs: `lib/jtty/sjtty.f90`, `lib/jtty/sjtty_qrm.f90`,
  `lib/jtty/rjtty.f90` and `lib/jtty/wava/`.
- Two files upstream's build does not compile and no code uses: `lib/jtty/jtty_codewords.f90`
  (an include fragment that nothing includes) and `lib/jtty/jtty_spec.f90` (named only in
  commented-out lines of `sjtty.f90`).
- Upstream's notes, read for facts and not copied: `lib/jtty/jtty_design.md`,
  `lib/jtty/jtty_source_encoding.txt`, `lib/jtty/jtty_n1mm_integration.md` and
  `lib/jtty/jtty_tx_manual_checks.md`. WSJT-X licenses its documentation separately, under
  CC BY-ND 4.0 (its `DOCS-LICENSE.md`), so Nexus writes its own.
- The tests that need WSJT-X's Qt code: `tests/unit/jtty/test_jtty_structured_decode.f90` with
  `tests/unit/jtty/jtty_cpp_n1mm_smoke.cpp`, and the C++ tests
  `tests/unit/jtty/test_jtty_live_entry.cpp`, `tests/unit/jtty/test_jtty_messages.cpp`,
  `tests/unit/jtty/test_jtty_n1mm.cpp`, `tests/unit/jtty/test_jtty_receive_history.cpp`,
  `tests/unit/jtty/test_jtty_receive_line.cpp`,
  `tests/unit/jtty/test_jtty_receive_results.cpp`, `tests/unit/jtty/test_jtty_recording.cpp`,
  `tests/unit/jtty/test_jtty_replay_ownership.cpp`, `tests/unit/jtty/test_jtty_spectrum.cpp`,
  `tests/unit/jtty/test_jtty_transmit_queue.cpp`, `tests/unit/jtty/test_jtty_tx_lifecycle.cpp`,
  `tests/unit/jtty/test_jtty_txstream.cpp`, `tests/unit/jtty/test_jtty_waterfall.cpp` and
  `tests/unit/jtty/test_mmttyif.cpp`, `tests/unit/jtty/test_jtty_filler.cpp`,
  `tests/unit/jtty/test_jtty_transmit_encoder.cpp`, `tests/unit/jtty/test_jtty_transmit_host.cpp`,
  `tests/unit/jtty/test_jtty_transmit_text.cpp`, `tests/unit/jtty/test_wsjt_codec_encode.cpp` and
  `tests/unit/jtty/test_wsjt_codec_jtty.cpp`; and upstream's build file
  `tests/unit/jtty/CMakeLists.txt`.
- Files added upstream in 3.3.0-beta2 to beta4 that carry a per-file licence header,
  `SPDX-License-Identifier: GPL-3.0-or-later`, unlike the files vendored here: the Fortran tests
  `tests/unit/jtty/test_jtty_genjtty_text.f90`, `tests/unit/jtty/test_jtty_display_layout.f90` and
  `tests/unit/jtty/test_jtty_final_window.f90`; `lib/jtty/jtty_filler.f90`, a port of the GUI's
  rule for live-entry filler in received text that only upstream's own receive code uses; and the
  C++ transmit code `lib/jtty/JttyTransmit.cpp`, `lib/jtty/JttyTransmit.hpp`,
  `lib/jtty/JttyTransmitHost.cpp`, `lib/jtty/JttyTransmitHost.hpp` and `lib/jtty/JttyUnicode.cpp`.
- WSJT-X's Qt/C++ JTTY code (in `widgets/`, `Modulator/` and the top-level `Jtty*` files), its
  integration tests (`tests/integration/jtty/`) and its sample recording (`samples/JTTY/`).

### Lineage

The JTTY files are from `v3.3.0-beta4`; everything they call from the rest of WSJT-X is a copy
already here, from WSJT-X 3.0.2: `pack28` and `unpack28` from `lib/77bit/packjt77.f90`, and
`lib/chkcall.f90`, `lib/four2a.f90`, `lib/twkfreq.f90`, `lib/db.f90`, `lib/ft2/gfsk_pulse.f90`
and `lib/fftw3mod.f90`. At `0f7d5928` upstream's copies of these are byte-identical
(`gfsk_pulse.f90` has moved to `lib/gfsk_pulse.f90`), except two: `twkfreq.f90` gained a fast
path for a constant frequency (upstream commit `32c04bc8`, 2026-09-04), and `packjt77.f90` was
rewritten. Nexus keeps its own modified 3.0.2 `packjt77.f90`; the JTTY files build against it
unchanged.

### Re-taking the files

At the WSJT-X general release that carries JTTY, in a WSJT-X clone at its tag,
`git ls-tree -r <tag> lib/jtty lib/77bit/packjt77_grammar.f90 tests/unit/jtty tests/fixtures/jtty`
prints every file with its blob id. In one commit: copy the files the *Vendored* list names
from that tag; set every upstream file in those directories in one of the two lists; update
the tag, the commit and the blob ids in `crates/tempo-fast-sys/tests/jtty_provenance.rs`, here
and in `NOTICE`; drop the pre-release paragraphs here and in `NOTICE` and the pre-release
clause of the README credit; and compare the routines under *Lineage* again.

## Third-party code with its OWN copyright (not the WSJT group's)

`lib/qra/` — **qracodes**, © 2016 Nico Palermo (IV3NWV) / Microtelecom Srl, **GPL-3.0-or-later**.
The Q-ary RA LDPC codec behind Q65: 9 C sources plus headers, per-file license headers RETAINED
VERBATIM. Unlike the rest of `lib/` (which carries no per-file headers and is therefore treated as
GPL-3.0-only), these files carry an explicit "or later" grant. Do not strip those headers.

⚠️ The same author's `lib/superfox/qpc/qpc_n127k50q128.c` is **NOT free software** ("licensed only
for use with WSJT-X") and is deliberately NOT vendored here. Per-file terms differ within one
author's work — determine license per FILE, never per author.

## What is original vs. reused

- **New protocol code by KD9TAW (2026)**, derived from the WSJT-X framework: the
  **TempoFast** 4-CPM turbo modem under `lib/tempofast/` (`gen_tempofast`,
  `gen_tempofastwave`, `turbo_decode_tempofast`, `cpm_trellis`, `bcjr_cpm`,
  `matched_filter_bank`, `tempofast_interleave`, `tempofast_demod_bcjr`,
  `tempofast_rv_detect`, `tempofast_sync`, `ir_harq_combine`, `ldpc348_91`,
  `tempofast_params`) and the TempoFast acquisition decoder
  `lib/tempofast_decode.f90`.

  *(TempoFast was named **FT1** before 2026-07-21, and the **TempoDeep** full-band
  acquisition modem — `libtempo/tempodeep/` — was named **DX1**. Only names changed;
  authorship, licensing and the on-air protocols are unaffected. Noted here so anyone
  tracing this source against an earlier release can follow it.)*
- **Reused from WSJT-X**, some files **modified by KD9TAW (2026)** to compile
  headlessly (no Qt, no shared memory, no streaming decode loop): the FT8 sources
  (`lib/ft8/`), FT4 sources (`lib/ft4/` + `lib/ft4_decode.f90`), 77-bit message
  packing (`lib/77bit/packjt77.f90`), the LDPC(174,91) / CRC / FFT infrastructure
  (`lib/ft8/*ldpc*`, `lib/chkcrc*`, `lib/crc*`, `lib/four2a.f90`,
  `lib/fftw3mod.f90`), and shared helpers at `lib/` top level.
- **Reused from WSJT-X unmodified, from a later release**: the JTTY modem
  (`lib/jtty/`, `lib/77bit/packjt77_grammar.f90`) and upstream's tests for it
  (`tests/`); see *JTTY — vendored from WSJT-X* above.

All files in this directory — original and modified — are distributed under
**GPL-3.0-only**, the same license as WSJT-X and as Nexus as a whole. See the
project `NOTICE` for the full dependency and lineage summary.
