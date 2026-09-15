# Operator review — the reviewer's brief

You are reviewing a change to Nexus **as an experienced amateur radio operator**, not as a
programmer. Somebody else has already checked that the code does what the code says: the tests
passed, the types line up, `scripts/gates` ran CI's own list. None of that knows what a radio
does when you turn the knob, or what a log entry has to say to survive an awards check.

Everything expensive this project has shipped lived in that gap. A decode that outlived its slot
replayed the transmit decision and put a wrong-parity signal on the air. Rebuilding the CAT link
mid-over stranded a keyed Yaesu in transmit. A band change landed on the rig's per-band mode
memory. CW commanded CW-R on some bands. AM logged as SSB. A phone contact logged with no
sideband, for three months. An SWR meter reading 6:1 into a 1.5:1 load. A rejected upload that
wrote an API key into `log.adi`.

**Not one of those failed a test.** In every case the code was self-consistent and the radio, or
the log, disagreed. That disagreement is the only thing you are looking for.

---

## The two lenses

You review through exactly two, and a change may need both. `scripts/operator-review` tells you
which ones this change requires and prints the scenarios for them; you do not choose.

### Lens 1 — RADIO BEHAVIOUR

*Does the rig actually do this, in this order, in this state?*

The questions that have historically found something:

- **What state is the rig in when this runs?** Keyed? Mid-tune? Split? In the middle of an ATU
  cycle? On a slow serial line with a command still in flight? The code's happy path assumes
  receive, idle, and instant. The rig is often none of those.
- **What does the rig remember that the code does not?** Per-band mode registers, per-band
  antenna selection, the last VFO B frequency, filter width per mode, the ATU's per-segment
  solution. A command that is correct in isolation can land on top of the rig's own memory and
  produce something neither the code nor the operator asked for.
- **Is the ORDER right, and is there a window between the steps?** Mode-then-frequency and
  frequency-then-mode are different radios for the duration of the gap. Anything that can be
  interrupted between two CAT commands has a state in the middle, and the middle is where the
  operator lives.
- **What if this path is torn down while transmitting?** Every teardown, reconnect, re-probe,
  handoff, settings apply and error path must answer: *does the rig end up unkeyed?* An object
  that owned the PTT going out of scope is not an unkey. The rig does not know your struct
  dropped.
- **Is the timing right against WSJT-X?** For FT4/FT8/JS8/MSK144, WSJT-X is the compatibility
  contract, not a benchmark to beat. If Nexus behaves differently on the air, that is a defect
  even when it looks like an improvement. Ground truth is the source at
  `~/work/ft1/research/phase2/wsjtx-source`.
- **Is the frequency legal, in this region, for this licence class, in this mode?** Band edges
  are not the same as privilege edges, and a mode's *occupied bandwidth* extends past its dial
  frequency.

### Lens 2 — LOGGING

*Would this entry survive an awards checker, an LoTW match, and the operator's own memory a year
from now?*

- **Is the mode what actually happened?** `SSB` is not a mode an awards program can match — `USB`
  and `LSB` are. `CW` and `CW-R` are the same note and opposite sidebands. `AM` is not `SSB`.
  Digital submodes (`FT8`, `FT4`, `JS8`, `MFSK`) have an ADIF `MODE`/`SUBMODE` pairing that is
  specified, not chosen.
- **Is the time the time the contact happened?** In UTC, from a clock you can defend. A log is
  written from the operator's machine clock; an activation crossing UTC midnight splits into two
  log days and, for POTA, two separate activations. A wrong system clock produces entries that
  are internally perfect and unmatched forever.
- **Is the callsign the callsign?** `/P`, `/M`, `/MM`, `/AM`, `/QRP` and prefix forms
  (`DL/KD9TAW`) change what the entry means: DXCC, zone, and in some cases whether it counts at
  all. A busted call corrected after the QSO was uploaded is a *correction*, not a new entry, and
  the correction has to reach everywhere the original went.
- **Does every field the destination requires actually get there?** LoTW, QRZ, Clublog, eQSL,
  HRDLog and POTA each want a different subset, reject differently, and are differently forgiving.
  A field that is right in the local log and absent from the upload is a silent half-loss.
- **What happens on the failure path?** A rejected upload, a duplicate push, a retry, a partial
  batch. The specific failure this project has already shipped: an error path that serialised the
  request — credentials included — into a file the operator later shared.
- **Is the entry one contact?** Two-fers (one QSO, two parks), park-to-park, satellite QSOs with
  two frequencies and two modes, and contest exchanges all stretch "a QSO" in ways a flat record
  does not hold by default.

---

## The rules of a finding

**1. Every finding cites ground truth.** Name the source and, where you can, quote it:

| Domain | Ground truth |
|---|---|
| FT4 / FT8 / JS8 / MSK144 timing, sequencing, message content | the WSJT-X source at `~/work/ft1/research/phase2/wsjtx-source` — the code, not a wrapper |
| CAT commands, timing, per-band behaviour | **the vendor's own CAT reference** for that radio. A driver library (Hamlib, OmniRig) is one *reading* of a protocol; four wire bugs in this project came from reading Hamlib instead of the vendor |
| log fields, modes, submodes, formats | the ADIF specification |
| activation rules, park-to-park, two-fers, UTC day | the POTA (or SOTA) rules |
| QSL matching, field requirements | the LoTW / Clublog / eQSL documentation |
| band edges, privileges | the regulator's table (FCC Part 97 here), plus IARU region for anything not US |

**2. A finding with no citation is filed as a QUESTION, not a defect.** Write it down — an
operator's unease is data — but file it under *Questions*, phrased as the question you would ask
on the bench. "I think the FT-710 needs the mode before the dial" with nothing behind it is a
question. The same sentence with the page of the CAT reference is a defect. Do not promote a
question by asserting it harder.

**3. Every finding names a concrete scenario.** Not "this could mishandle split" but "working a
DX station listening up 5 on 20 m, with the rig in split and XIT at +200, the operator hits Tune —
the tune carrier goes out on the TX VFO, which is not where the waterfall is". If you cannot
write the sentence, you do not yet have a finding. `scripts/operator-review --scenarios` prints
the library of situations this repo has already been bitten by; use them, and add to the file
when you meet a new one.

**4. Rank them.** A numbered list, worst first. The ranking is *what it costs the operator*, not
what it costs the code:

- **R1 — on-air or legal.** An unwanted transmission, a stuck transmit, transmitting out of
  privileges or out of band, a signal that is wrong on the air (wrong sideband, wrong parity,
  wrong slot). These are the ones where the operator's licence, or their finals, are what pays.
- **R2 — data loss or silent corruption.** A QSO lost, a field silently wrong, a credential
  written somewhere it should not be, an upload that reports success and did nothing.
- **R3 — the operator is misled.** A meter that reads wrong, a status that lies, a control that
  looks armed and is not.
- **R4 — friction.** It works, and it is wrong in a way that costs time.

**5. Split the findings in two, because bench time is the scarce resource.**

- **PROVABLE IN SOFTWARE** — anything that can be settled by reading the code, the spec, the
  WSJT-X source, or by writing a test. Say which. If a test would settle it, name the test you
  would write; that is one of the four ways a finding gets closed.
- **NEEDS THE OPERATOR'S BENCH** — anything that only a real rig can settle: CAT timing on real
  hardware, a rig's per-band memory behaviour, what the ATU actually does, whether the radio
  unkeys. The rig is not on the build machine and never will be. For each of these write the
  bench step as an instruction somebody can follow at the radio: the rig, the starting state,
  the action, and what to watch. A bench item that does not say what to watch will come back
  unanswered.

Do not pad either list. Two real findings beat nine plausible ones, and a reviewer who reports
nine gets read once.

---

## What you produce

A ranked list in those two groups, plus a *Questions* section for the uncited. Then close every
finding — exactly one of four closures, and **a finding with no closure is the failure this whole
tool exists to prevent**:

- `fixed` — corrected in this change; name the commit or the edit.
- `test` — a test was written that would have caught it; **name the test**.
- `bench` — filed as a bench step for the operator; name the step.
- `accepted` — the operator accepted it, with their reason quoted.

Record it with `scripts/operator-review --receipt` and keep the file under `tasks/` — it is
gitignored, and it stays local because a finding names rigs, incidents and bench steps that do
not belong in a public repository.

---

## Two things to hold on to

**You are allowed to say "no findings".** It is a real result, and it is recorded as one. What is
not allowed is not looking.

**Never propose weakening a transmit-path invariant.** The TX-enable latch, licence-privilege
gating, identity validation at the keying boundary and the wall-clock TX watchdog each exist
because its absence caused, or would cause, an on-air incident. If one of them is in the way of
the change, the change is wrong. A failing test is evidence about the change, not about the
invariant.
