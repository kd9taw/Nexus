# Vendored WSJT-X–derived modem source (`libtempo`)

This directory is the **complete corresponding source** (GPL-3.0-only, §6) for
the FT8 / FT4 / FT1 modem that `libtempo` compiles into the Nexus binary. It is
vendored in-tree so that a fresh clone builds the modem with **no external
checkout** — `libtempo/CMakeLists.txt` points `WX` here by default.

## Provenance and license

The DSP sources here are derived from **WSJT-X**, © Joe Taylor (K1JT) and the
WSJT Development Group (Steve Franke K9AN, Bill Somerville G4WJS, Nico Palermo
IV3NWV, and others), licensed under the **GNU GPL, version 3** (GPL-3.0-only; the vendored `lib/` files carry no per-file license headers, so no "or later" grant applies).

- Upstream project: <https://sourceforge.net/projects/wsjt/>
- Full license text: `COPYING` at the repository root.

This is a **subset** of WSJT-X's `lib/` — the Fortran/C/C++ DSP sources that `libtempo`
actually builds, and nothing else. Measured against this tree (2026-08-07): **188 source
files** (158 `.f90`, 28 `.c`, 2 `.cpp`) plus 22 headers. 167 of the 188 are named one by one
in `libtempo/CMakeLists.txt` — the list is enumerated, never globbed — and the remaining 21
are the parameter/generator/parity/table files those sources pull in with Fortran `include`,
so every file here is corresponding source for something the binary contains.

**None of WSJT-X's Qt/GUI code is included, and that claim is about THIS DIRECTORY.**
Measured: zero occurrences of `#include <Q…>`, `QString`, `QRegularExpression`, `QObject`,
`Q_OBJECT` or `QCoreApplication` across all 211 files, and zero `.ui`, `.qrc`, `.pro`,
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

All files in this directory — original and modified — are distributed under
**GPL-3.0-only**, the same license as WSJT-X and as Nexus as a whole. See the
project `NOTICE` for the full dependency and lineage summary.
