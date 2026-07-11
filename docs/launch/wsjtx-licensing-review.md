# WSJT-X Code in Nexus — Licensing Review (2026-07-11)

Prompted by a community claim that "someone tried to embed WSJT-X into Ham Radio Deluxe
in the early 2000s and got sued." Research: two independent source-cited sweeps (HRD
history + WSJT enforcement history) plus a file-level audit of this repository.
*This is an engineering licensing review, not legal advice.*

## The claim itself is false

- **The timeline is impossible.** WSJT-X first appeared ~2013 and FT8 in 2017; HRD dates
  to ~2003. Nothing WSJT-X could have been embedded in anything "in the early 2000s."
- **No lawsuit between HRD and WSJT/K1JT has ever existed** — no suit, C&D, DMCA, or
  takedown appears in ARRL coverage, project archives, or press. HRD's own support page
  ("Why Don't We Add FT8 & FT4 into DM-780?") says the opposite: they **declined** to
  embed WSJT modes *because* GPLv3 would obligate them to open-source HRD — the license
  was a wall they chose not to cross, and their solution is external WSJT-X feeding HRD
  Logbook.
- **What people are remembering** is almost certainly a fusion of (a) that oft-repeated
  "HRD can't add FT8 because of the GPL" explanation and (b) HRD Software's unrelated
  2016 license-blacklisting scandal (revoking keys over negative reviews; Techdirt/The
  Register). "HRD + GPL + legal trouble" compressed into a lawsuit that never happened.

## Why Nexus is not the HRD scenario

The thing that would have made an HRD embedding unlawful — **closed-source commercial
software absorbing GPL code without releasing source** — is the exact opposite of
Nexus's posture:

| | HRD (hypothetical embedding) | Nexus (actual) |
|---|---|---|
| License of the host app | Proprietary, commercial | **GPL-3.0, free** |
| Source availability | Closed | **Published (SourceForge + GitHub)** |
| WSJT-X-derived source | Would be absorbed, hidden | **Vendored in-tree** (`libft1/vendor/wsjtx/`, release trees) as complete corresponding source |
| Modifications | — | **Marked** (GPLv3 §5(a)), provenance in the vendor README |
| Attribution | — | K1JT + the WSJT Development Group credited in NOTICE, the vendor README, and source comments |

GPL code inside a GPL program with source published is not a gray area — it is the
license operating as designed, and it is the same basis on which MSHV, JTDX, and other
WSJT-X derivatives have operated publicly for years.

## What Nexus actually takes from WSJT-X (audited)

1. **Compiled source (the GPL-relevant part):** ~70 Fortran/C DSP files from WSJT-X's
   `lib/` (FT8/FT4 codecs, 77-bit packing, LDPC(174,91), CRC, FFT plumbing) compiled
   into `libft1`. Conveyed under GPLv3 with provenance, marked modifications, and the
   complete corresponding source vendored in release trees.
2. **Protocol compatibility (no license attaches):** the FT8/FT4 wire protocols are
   openly published (Franke/Taylor et al., QEX) and protocols/interfaces are functional
   subject matter — independent implementation is lawful (cf. *Google v. Oracle*, 2021).
   The WSJT-X UDP protocol Nexus speaks to loggers is in this category.
3. **Behavioral reimplementation (no license attaches):** Nexus's auto-sequencer
   (`qso.rs`) is original Rust modeled on WSJT-X's observed behavior; no code copied.
4. **Names:** "FT8", "FT4", "WSJT-X" are used descriptively (nominative use). No
   registered trademarks were found on any of them, and no one has ever been asked to
   stop using the names. Nexus does not present itself as official WSJT-X.

## The counterparty's actual enforcement history

- **Zero legal actions, ever** — no lawsuit, DMCA, C&D, or forge takedown by the WSJT
  group against any fork, embedder, or commercial product, across the project's history.
- The most adversarial episode on record is a **2017 mailing-list post by K1JT about
  JTDX** — objecting to a misleading window title, over-claimed credit, missing GPL
  notices, and no public repo. Resolved cooperatively on the list. Every concern raised
  there is one Nexus already satisfies.
- Their stated asks of derivatives are **normative**: don't pass yourself off as
  official WSJT-X, credit the authors, and don't ship Release-Candidate-only features
  before their GA release (backed by "we'll stop publishing RCs," not by lawyers).
- **KVASD precedent:** WSJT-X itself once cooperated at arm's length with a proprietary
  decoder binary (fetched separately, never bundled) until replacing it with the open
  Franke-Taylor decoder in v1.7.0 — a project that engineers *its own* GPL boundaries
  carefully and has never litigated anyone else's.

## Version nuance found and fixed by this review

WSJT-X is **GPLv3 (v3-only by Debian's classification)** and its `lib/` sources carry no
"or any later version" boilerplate — so Nexus cannot claim "GPL-3.0-or-later" for the
derived work or the combined whole. Corrected in this pass: NOTICE and the workspace
license metadata now say **GPL-3.0-only** for the combined work; the vendor README's
license line gets the same correction at the next release snapshot. (Compatibility is
unaffected either way; this is precision, not exposure.)

Also hardened in this pass: the Windows installer now bundles `COPYING` + `NOTICE`
beside the app (GPL §4/§6 — the license travels with the binary), and NOTICE was
updated to reflect that the corresponding source is vendored in release trees.

## Behavioral guardrails going forward

- Never present Nexus as official WSJT-X or imply endorsement; keep crediting K1JT and
  the WSJT Development Group wherever the modem is described.
- Don't port features that exist only in WSJT-X Release Candidates before their GA.
- Keep the vendor README + NOTICE lineage current whenever `libft1` changes.
- If a specific legal threat (not forum talk) ever arrives, take it to counsel with
  this memo and the NOTICE/vendor-README lineage in hand.
