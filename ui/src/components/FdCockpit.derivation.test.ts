// THE POSITION'S MODE CLASS, and why it gets a test of its own.
//
// `fdModeClassFromRig` decides two things that cannot be corrected after the fact: which cell
// of the band × mode board a station is checked against (so whether the operator is told "new"
// about someone the engine is about to refuse), and — through `LogEntry`'s `fdMode` — which
// class the contact is submitted under. A misclassification is a wrong dupe verdict during the
// run and a wrong Cabrillo line after it.
//
// So the test is not a handful of examples. It READS THE RUST that produces the strings and
// requires an expectation for every one of them: a mode name added on the radio side with no
// entry here FAILS, rather than silently falling through to the function's `else DIG`.
//
// Three Rust sources, and together they are the union of what can reach `radio.rigMode`:
//   `civ/commands.rs`   — the native CI-V backend's canonical names AND the aliases it accepts
//                         (`Mode::name` / `Mode::from_name`).
//   `omnirig/mod.rs`    — the OmniRig shim's rigctld vocabulary in both directions
//                         (`from_rigctld` / `to_rigctld`), which is where the DATA sub-mode
//                         spellings and the FM-data names live.
//   `civ/broker.rs`     — the CI-V daemon's own read-back, which reports a DATA sub-mode as
//                         PKTUSB/PKTLSB/PKTFM when the rig's DATA switch is on.
// The fourth producer is Hamlib itself (a rigctld `m` read passes its answer through verbatim,
// `Engine::observe_rig_mode`); its catalog is not a Rust file in this tree, so it is written
// out below and marked as such.
import { describe, it, expect } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { fdModeClassFromRig } from './FdCockpit'

// The variable indirection is deliberate: Vite rewrites a LITERAL `new URL('./x', import.meta.url)`
// into a served asset URL, and `fileURLToPath` then throws "The URL must be of scheme file".
const read = (rel: string) => readFileSync(fileURLToPath(new URL(rel, import.meta.url)), 'utf8')
const CIV_RS = read('../../../crates/tempo-audio/src/civ/commands.rs')
const OMNIRIG_RS = read('../../../crates/tempo-audio/src/omnirig/mod.rs')
const BROKER_RS = read('../../../crates/tempo-audio/src/civ/broker.rs')

/** The source between two markers. Throws when either is gone — a rename must fail loudly
 *  rather than quietly leaving this guard scanning nothing. */
function span(src: string, where: string, from: string, to: string): string {
  const a = src.indexOf(from)
  if (a < 0) throw new Error(`${where}: "${from}" not found`)
  const b = src.indexOf(to, a)
  if (b < 0) throw new Error(`${where}: "${to}" not found after "${from}"`)
  return src.slice(a, b)
}

/** Every uppercase mode token quoted inside a span. Mode names are the only all-caps quoted
 *  strings in these two mapping blocks. */
function modeTokens(text: string): string[] {
  return [...text.matchAll(/"([A-Z][A-Z0-9_-]*)"/g)].map((m) => m[1])
}

/** What the CI-V backend names a mode, and every alias it will accept for one. */
const CIV_NAMES = modeTokens(
  span(CIV_RS, 'civ/commands.rs', 'pub fn name(self)', '// ---- command builders'),
)
/** The OmniRig shim's rigctld vocabulary, both directions. */
const OMNIRIG_NAMES = modeTokens(
  span(OMNIRIG_RS, 'omnirig/mod.rs', 'pub fn from_rigctld', '**The COM boundary.**'),
)
/** What the CI-V daemon REPORTS for a rig sitting in a DATA sub-mode. */
const BROKER_NAMES = modeTokens(
  span(BROKER_RS, 'civ/broker.rs', 'let name = match (mode, st.data_mode', 'fn ptt(&self)'),
)

/**
 * Hamlib's own mode catalog — what a rigctld `m` read can answer with on a rig this project
 * does not have a native backend for. NOT read from a file: Hamlib is a vendored C dependency
 * and its `rig_MODE` table is not a Rust source in this tree. Written out and marked, so the
 * next person can see it is a hand-maintained half rather than assume it is computed.
 */
const HAMLIB_NAMES = [
  'USB', 'LSB', 'CW', 'CWR', 'RTTY', 'RTTYR', 'AM', 'FM', 'WFM', 'FMN', 'AMN',
  'AMS', 'SAM', 'SAL', 'SAH', 'DSB', 'ECSSUSB', 'ECSSLSB', 'FAX', 'PKTLSB', 'PKTUSB',
  'PKTFM', 'PKTAM', 'PSK', 'PSKR', 'P25', 'DSTAR', 'DPMR', 'NXDNVN', 'NXDN_N', 'DCR',
  'DD', 'C4FM', 'SPEC',
]

/**
 * THE RULING, one line per mode name. PH is voice, CW is CW, and everything else an FD entry
 * can be made on is DIG — including every DATA sub-mode, because a DATA sub-mode is what a rig
 * sits in while the sound card works FT8, RTTY, PSK or SSTV.
 *
 * A name here that the sources no longer produce is harmless; a name they produce that is NOT
 * here fails the census below. That asymmetry is on purpose — this list is allowed to be
 * generous, never short.
 */
const EXPECTED: Record<string, 'CW' | 'PH' | 'DIG'> = {
  // voice
  USB: 'PH', LSB: 'PH', SSB: 'PH', DSB: 'PH',
  AM: 'PH', AMN: 'PH', AMS: 'PH', SAM: 'PH', SAL: 'PH', SAH: 'PH',
  ECSSUSB: 'PH', ECSSLSB: 'PH',
  FM: 'PH', FMN: 'PH', WFM: 'PH',
  // CW, both polarities and every spelling of the reverse
  CW: 'CW', CWR: 'CW', 'CW-R': 'CW', CWL: 'CW', 'CW-L': 'CW', CWU: 'CW', 'CW-U': 'CW',
  // data sub-modes: the rig is in a voice emission but the sound card is on the air
  PKTUSB: 'DIG', PKTLSB: 'DIG', PKTFM: 'DIG', PKTAM: 'DIG',
  'DATA-U': 'DIG', 'DATA-L': 'DIG', 'PKT-U': 'DIG', 'PKT-L': 'DIG', 'PKT-FM': 'DIG',
  'USB-D': 'DIG', 'LSB-D': 'DIG', 'FM-D': 'DIG',
  // modes that are digital in their own right
  RTTY: 'DIG', RTTYR: 'DIG', 'RTTY-R': 'DIG', FSK: 'DIG', FSKR: 'DIG',
  PSK: 'DIG', PSKR: 'DIG', DD: 'DIG', FAX: 'DIG', SPEC: 'DIG',
  P25: 'DIG', DSTAR: 'DIG', DPMR: 'DIG', NXDNVN: 'DIG', NXDN_N: 'DIG', DCR: 'DIG',
  C4FM: 'DIG',
}

describe('the mode-class derivation covers every mode string the radio layer emits', () => {
  it('the extraction actually found the mapping tables (the positive control)', () => {
    // If a rename silently emptied a span, every census below would pass over nothing.
    expect(CIV_NAMES, 'civ/commands.rs mode names not extracted').toContain('CWR' as never)
    expect(CIV_NAMES).toContain('RTTYR' as never)
    expect(CIV_NAMES).toContain('FSK' as never)
    expect(OMNIRIG_NAMES, 'omnirig mode names not extracted').toContain('DATA-U' as never)
    expect(OMNIRIG_NAMES).toContain('PKTFM' as never)
    expect(BROKER_NAMES, 'civ/broker.rs mode names not extracted').toContain('PKTUSB' as never)
    expect(CIV_NAMES.length).toBeGreaterThan(10)
    expect(OMNIRIG_NAMES.length).toBeGreaterThan(10)
    expect(BROKER_NAMES.length).toBeGreaterThan(2)
  })

  const ALL = [...new Set([...CIV_NAMES, ...OMNIRIG_NAMES, ...BROKER_NAMES, ...HAMLIB_NAMES])].sort()

  it('every emitted name has a ruling — a new mode on the radio side fails here', () => {
    const unruled = ALL.filter((m) => !(m in EXPECTED))
    expect(
      unruled,
      'the radio layer can report these mode names and nothing says what Field Day class ' +
        'they are. They currently fall through to DIG by default, which may be wrong — add ' +
        'each to EXPECTED (and to fdModeClassFromRig when it is), rather than to this list.',
    ).toEqual([])
  })

  for (const name of ALL) {
    it(`${name} → ${EXPECTED[name] ?? '(unruled)'}`, () => {
      expect(fdModeClassFromRig(name)).toBe(EXPECTED[name])
    })
  }

  it('reads the case the rig sent, whatever case that is', () => {
    // Hamlib is uppercase, the CI-V shim uppercases, and OmniRig round-trips — but
    // `observe_rig_mode` only trims, so a backend that ever answers lowercase must not
    // reclassify the whole position.
    expect(fdModeClassFromRig('pktusb')).toBe('DIG')
    expect(fdModeClassFromRig('  usb  ')).toBe('PH')
    expect(fdModeClassFromRig('cw')).toBe('CW')
  })

  it('a data sub-mode is DIG even though its name contains a sideband', () => {
    // The order of the tests inside the function is the whole of this. The prefix forms are
    // held by the voice rule's `^` anchor as well; it is the SUFFIX forms (USB-D, LSB-D, FM-D)
    // that a sideband test running first would claim as voice — measured, by swapping the two
    // rules and watching exactly those three go red.
    for (const m of ['PKTUSB', 'PKTLSB', 'DATA-U', 'DATA-L', 'USB-D', 'LSB-D', 'FM-D', 'PKTFM', 'PKTAM']) {
      expect(fdModeClassFromRig(m), `${m} classified as voice`).toBe('DIG')
    }
  })
})

describe('the fallback when the radio has not answered', () => {
  // `rigMode` is None until a CAT read succeeds (`Engine::observe_rig_mode`), and a Field Day
  // station with no CAT at all is an ordinary Field Day station — so the engine's own
  // operating SECTION carries the derivation instead of it defaulting to one class forever.
  it('falls back to the engine operating section', () => {
    expect(fdModeClassFromRig(null, 'cw')).toBe('CW')
    expect(fdModeClassFromRig(undefined, 'phone')).toBe('PH')
    expect(fdModeClassFromRig('', 'digital')).toBe('DIG')
    expect(fdModeClassFromRig('', 'rtty')).toBe('DIG')
    expect(fdModeClassFromRig('', 'keyboard')).toBe('DIG')
  })

  it('the rig wins over the section — the rig is what is on the air', () => {
    // The operator left the section on Phone and moved the rig to CW: the contact that goes
    // in the log is a CW contact.
    expect(fdModeClassFromRig('CW', 'phone')).toBe('CW')
    expect(fdModeClassFromRig('PKTUSB', 'phone')).toBe('DIG')
    expect(fdModeClassFromRig('USB', 'digital')).toBe('PH')
  })

  it('nothing at all is DIG, not a crash and not a blank', () => {
    expect(fdModeClassFromRig(null)).toBe('DIG')
    expect(fdModeClassFromRig(undefined, undefined)).toBe('DIG')
  })
})
