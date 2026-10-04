// Synthetic spectrum frames for the real-browser harness — pure and deterministic, so a frame is
// the same bytes on every machine and every run.
//
// TWO KINDS, because the probes ask two different questions:
//
//   * INDEXED sets (the pixel fixtures) are served by CALL: the n-th row a component asks for is
//     row n, whatever the clock says. The picture after N rows is then a function of the code
//     alone, which is what lets a stored PNG be compared at a tight tolerance.
//   * TIMED sets (the cadence and perf probes) are served by the REAL CLOCK, the way the engine's
//     latest-value slot serves them: a component that asks twice between two sweeps gets the same
//     sweep twice. Each sweep carries a barcode of its own sequence number, so the cadence probe
//     can read back off the canvas which sweep every committed waterfall row shows.
//
// Values follow the row contract the components read (`Spectrum` in ui/src/types.ts): 0..1, and
// on the audio feed linear in dB with 0 = −120 dBFS (`WF_DB_SPAN`). The native feeds keep their
// own scale and resolution: CI-V 475 points of byte/160, Flex 2048 bins of bin/65535, FT-710
// 850 bins.

export interface Frame {
  row: number[]
  loHz: number
  hiHz: number
  source: string
}

/** Row value for a level in dBFS on the audio feed's axis (0..1 = −120..0 dBFS). */
export function dbfs(db: number): number {
  return Math.min(1, Math.max(0, (db + 120) / 120))
}

/** mulberry32 — a small seeded PRNG, so "noise" is the same noise everywhere. */
export function prng(seed: number): () => number {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

/** A noise floor in dB: roughly normal around `floorDb` (sum of three uniforms, sd ≈ 2.5 dB). */
function noiseDb(bins: number, floorDb: number, seed: number): Float64Array {
  const r = prng(seed)
  const out = new Float64Array(bins)
  for (let b = 0; b < bins; b++) out[b] = floorDb + 5 * (r() + r() + r() - 1.5)
  return out
}

/** Power-add a tone at `hz` into a dB row spanning `loHz..hiHz` (bin cells), with a Hann-like
 *  main lobe: −6 dB one bin out, −24 dB two bins out, −54 dB three bins out. */
function addTone(rowDb: Float64Array, loHz: number, hiHz: number, hz: number, levelDb: number): void {
  const bins = rowDb.length
  const p = ((hz - loHz) / (hiHz - loHz)) * bins - 0.5
  for (let b = Math.floor(p) - 3; b <= Math.ceil(p) + 3; b++) {
    if (b < 0 || b >= bins) continue
    const d = Math.abs(b - p)
    const rel = -6.02 * d * d
    if (rel < -60) continue
    const sum = 10 ** (rowDb[b] / 10) + 10 ** ((levelDb + rel) / 10)
    rowDb[b] = 10 * Math.log10(sum)
  }
}

const AUDIO_BINS = 512
const AUDIO_LO = 0
const AUDIO_HI = 4000

function audioFrame(rowDb: Float64Array): Frame {
  return { row: Array.from(rowDb, dbfs), loHz: AUDIO_LO, hiHz: AUDIO_HI, source: 'audio' }
}

// ---------------------------------------------------------------------------------------------
// INDEXED sets — the pixel fixtures.

export interface IndexedSet {
  id: string
  /** Rows the fixture defines; asks past the end get the last row again (see `PAD_ROWS`). */
  count: number
  frame(i: number): Frame
}

/**
 * Rows served after a fixture's last row, identical to it. The rig scope's trace is a peak hold
 * that decays with REAL elapsed time between rows, so a trace drawn mid-change depends on the
 * clock. Forty identical rows at 50 ms take the decay to e^−8 of any earlier peak under the
 * shortest hold the scope ships, so the trace at capture is the last row's own shape on every run.
 */
export const PAD_ROWS = 40

/** FT8's Costas sync array, at symbols 0, 36 and 72 of 79. */
const COSTAS = [3, 1, 4, 0, 6, 5, 2]

export const INDEXED_SETS: Record<string, IndexedSet> = {
  /** One carrier 50 dB over a −95 dBFS floor. */
  carrier: {
    id: 'carrier',
    count: 80,
    frame(i) {
      const row = noiseDb(AUDIO_BINS, -95, 1000 + i)
      addTone(row, AUDIO_LO, AUDIO_HI, 1000, -45)
      return audioFrame(row)
    },
  },
  /** The classic two-tone test: 700 Hz and 1900 Hz at equal level. */
  'two-tone': {
    id: 'two-tone',
    count: 80,
    frame(i) {
      const row = noiseDb(AUDIO_BINS, -95, 2000 + i)
      addTone(row, AUDIO_LO, AUDIO_HI, 700, -50)
      addTone(row, AUDIO_LO, AUDIO_HI, 1900, -50)
      return audioFrame(row)
    },
  },
  /** The floor steps up 15 dB halfway; a weak carrier 15 dB over the first floor sinks into the
   *  second. What the visual AGC does with a band that suddenly gets noisier. */
  'noise-step': {
    id: 'noise-step',
    count: 120,
    frame(i) {
      const row = noiseDb(AUDIO_BINS, i < 60 ? -100 : -85, 3000 + i)
      addTone(row, AUDIO_LO, AUDIO_HI, 1500, -85)
      return audioFrame(row)
    },
  },
  /** One FT8 period at the FT waterfall's 120 ms row: three 8-FSK signals (6.25 Hz tones, 79
   *  symbols of 160 ms from 0.5 s) at different levels, then the quiet tail of the period. */
  'ft8-slot': {
    id: 'ft8-slot',
    count: 125,
    frame(i) {
      const t = i * 0.12
      const row = noiseDb(AUDIO_BINS, -95, 4000 + i)
      const signals = [
        { f0: 600, db: -80, seed: 11 },
        { f0: 1200, db: -70, seed: 12 },
        { f0: 1850, db: -85, seed: 13 },
      ]
      for (const s of signals) {
        const sym = Math.floor((t - 0.5) / 0.16)
        if (sym < 0 || sym >= 79) continue
        const k = sym % 36
        const tone = k < 7 ? COSTAS[k] : Math.floor(prng(s.seed * 1000 + sym)() * 8)
        addTone(row, AUDIO_LO, AUDIO_HI, s.f0 + 6.25 * tone, s.db)
      }
      return audioFrame(row)
    },
  },
}

/**
 * Sets only the renderer fixtures use (the renderer is fed rows directly, so they need no padding).
 * They ask what the components' fixtures cannot: history drawn through each row's own span across
 * a retune, a native 2048-bin source decimated by each detector, and a one-row burst the 3-D stack
 * must reject.
 */
export const RENDERER_SETS: Record<string, IndexedSet> = {
  /** A QSY halfway: the span moves up 500 Hz while two carriers stay at 1500 and 3000 Hz absolute.
   *  Drawn through each row's own span, they stay straight lines across the step. */
  retune: {
    id: 'retune',
    count: 120,
    frame(i) {
      const lo = i < 60 ? 0 : 500
      const row = noiseDb(AUDIO_BINS, -95, 5000 + i)
      addTone(row, lo, lo + 4000, 1500, -50)
      addTone(row, lo, lo + 4000, 3000, -60)
      return { row: Array.from(row, dbfs), loHz: lo, hiHz: lo + 4000, source: 'audio' }
    },
  },
  /** A Flex-width row: 2048 bins over 200 kHz, three carriers and one 8-bin-wide signal, so a
   *  pixel covers about five bins and the detectors disagree. */
  wide: {
    id: 'wide',
    count: 80,
    frame(i) {
      const lo = 14_000_000
      const hi = 14_200_000
      const row = noiseDb(2048, -100, 6000 + i)
      for (const [hz, db] of [
        [14_030_000, -45],
        [14_074_000, -60],
        [14_150_000, -75],
      ]) addTone(row, lo, hi, hz, db)
      for (let k = 0; k < 8; k++) addTone(row, lo, hi, 14_110_000 + k * 100, -70)
      return { row: Array.from(row, dbfs), loHz: lo, hiHz: hi, source: 'flex' }
    },
  },
  /** The audio passband, then a 23 cm rig scope (5 kHz across 1296.1 MHz, 475 points): far enough
   *  from where the history began that float32 hertz would put a carrier pixels off, unless the
   *  renderer re-bases its frequencies on the newest rows. */
  uhf: {
    id: 'uhf',
    count: 120,
    frame(i) {
      if (i < 40) return INDEXED_SETS.carrier.frame(i)
      const lo = 1_296_097_500
      const hi = 1_296_102_500
      const row = noiseDb(475, -95, 8000 + i)
      addTone(row, lo, hi, 1_296_100_000, -50)
      addTone(row, lo, hi, 1_296_099_000, -60)
      return { row: Array.from(row, dbfs), loHz: lo, hiHz: hi, source: 'civ' }
    },
  },
  /** Identical rows (one noise draw) with a two-tone: the stack's median of three leaves them be. */
  steady: {
    id: 'steady',
    count: 60,
    frame() {
      const row = noiseDb(AUDIO_BINS, -95, 7000)
      addTone(row, AUDIO_LO, AUDIO_HI, 900, -50)
      addTone(row, AUDIO_LO, AUDIO_HI, 2600, -55)
      return audioFrame(row)
    },
  },
  /** `steady` with row 40 a broadband burst 55 dB over the floor: one row of interference. */
  'steady-burst': {
    id: 'steady-burst',
    count: 60,
    frame(i) {
      if (i !== 40) return RENDERER_SETS.steady.frame(i)
      return audioFrame(new Float64Array(AUDIO_BINS).fill(-40))
    },
  },
}

/** Row `i` of an indexed set, padded with the set's last row (see `PAD_ROWS`). */
export function indexedFrame(set: IndexedSet, i: number): Frame {
  return set.frame(Math.min(i, set.count - 1))
}

// ---------------------------------------------------------------------------------------------
// TIMED sets — the cadence and perf probes.

export interface TimedSet {
  id: string
  /** New frames per second at the producer. */
  rate: number
  bins: number
  loHz: number
  hiHz: number
  source: string
  /** The producer's own quantisation of a 0..1 value (CI-V publishes byte/160). */
  quantum: number
}

/**
 * The feeds Nexus has today, at the rates their producers publish.
 *
 * ⚠️ THE CI-V RATE IS NOT MEASURED. No radio has been timed: 3 sweeps a second is the cadence probe's
 * own planning figure, and ~10 is an estimate from the serial load of the waveform (about 7.5 KB/s
 * at 115200 baud). Both are here so the probe reports the defect at either; replace them with the
 * IC-9700's measured rate when it has one. The FT-710 (84/s) and Flex (15/s) rates are the
 * producers' own.
 */
export const TIMED_SETS: Record<string, TimedSet> = {
  'audio-50': { id: 'audio-50', rate: 50, bins: AUDIO_BINS, loHz: AUDIO_LO, hiHz: AUDIO_HI, source: 'audio', quantum: 0 },
  'civ-3': { id: 'civ-3', rate: 3, bins: 475, loHz: 144_975_000, hiHz: 145_025_000, source: 'civ', quantum: 1 / 160 },
  'civ-10': { id: 'civ-10', rate: 10, bins: 475, loHz: 144_975_000, hiHz: 145_025_000, source: 'civ', quantum: 1 / 160 },
  'flex-15': { id: 'flex-15', rate: 15, bins: 2048, loHz: 14_000_000, hiHz: 14_200_000, source: 'flex', quantum: 1 / 65535 },
  'ft710-84': { id: 'ft710-84', rate: 84, bins: 850, loHz: 14_024_000, hiHz: 14_124_000, source: 'yaesu', quantum: 0 },
}

/** The barcode: slot 0 is always strong and slot 1 always weak (the sync), then 14 bits of the
 *  sweep number, most significant first, then a 4-bit check. A row whose sync is inverted and
 *  whose other slots are all strong is the TERMINATOR, served once when a cadence window ends. */
export const CODE_SLOTS = 20
const SEQ_BITS = 14
const CHECK_BITS = 4
const STRONG = 0.92
const FLOOR = 0.22

/** The XOR of the sweep number's nibbles: any single misread bit, in the number or the check,
 *  changes it. */
function checkOf(seq: number): number {
  return (seq ^ (seq >> 4) ^ (seq >> 8) ^ (seq >> 12)) & 0xf
}

/** The strong/weak pattern of each slot for a sweep number, or for the terminator (`null`). */
export function codeBits(seq: number | null): boolean[] {
  if (seq == null) return [false, ...new Array(CODE_SLOTS - 1).fill(true)]
  const bits = [true, false]
  for (let k = SEQ_BITS - 1; k >= 0; k--) bits.push(((seq >> k) & 1) === 1)
  const c = checkOf(seq)
  for (let k = CHECK_BITS - 1; k >= 0; k--) bits.push(((c >> k) & 1) === 1)
  return bits
}

/** Read a slot pattern back: the sweep number, `'end'` for the terminator, or null. */
export function decodeBits(bits: boolean[]): number | 'end' | null {
  if (bits.length !== CODE_SLOTS) return null
  if (!bits[0] && bits[1] && bits.slice(2).every(Boolean)) return 'end'
  if (!bits[0] || bits[1]) return null
  let seq = 0
  for (let k = 0; k < SEQ_BITS; k++) seq = (seq << 1) | (bits[2 + k] ? 1 : 0)
  let c = 0
  for (let k = 0; k < CHECK_BITS; k++) c = (c << 1) | (bits[2 + SEQ_BITS + k] ? 1 : 0)
  return c === checkOf(seq) ? seq : null
}

/** Centre of slot `s` as a fraction of the row's span. Slots fill the middle 90% of the row,
 *  each taking half its pitch, so at least half of every stretch of the row is floor: the scope's
 *  median floor and the FT waterfall's 10th-percentile flattener both still land on the noise. */
export function slotCentre(s: number): number {
  return 0.05 + (0.9 * (s + 0.5)) / CODE_SLOTS
}

function quantise(v: number, q: number): number {
  return q > 0 ? Math.round(v / q) * q : v
}

/** One sweep of a timed set. `coded` stamps the barcode (cadence); without it the sweep is a
 *  floor with three carriers, which is what the perf probe draws. `seq` null = the terminator. */
export function timedFrame(set: TimedSet, seq: number | null, coded: boolean): Frame {
  const n = set.bins
  const r = prng(((seq ?? 0x3fff) + 1) * 7919 + n)
  const row = new Array<number>(n)
  for (let b = 0; b < n; b++) row[b] = FLOOR + 0.03 * (r() + r() - 1)
  if (coded) {
    const bits = codeBits(seq)
    const half = Math.max(2, Math.floor((0.9 * n) / CODE_SLOTS / 4))
    for (let s = 0; s < CODE_SLOTS; s++) {
      if (!bits[s]) continue
      const c = Math.round(slotCentre(s) * n - 0.5)
      for (let b = c - half; b <= c + half; b++) if (b >= 0 && b < n) row[b] = STRONG
    }
  } else {
    for (const at of [0.23, 0.51, 0.77]) {
      const c = Math.round(at * n)
      for (let b = c - 2; b <= c + 2; b++) row[b] = Math.max(row[b], 0.75 - 0.08 * Math.abs(b - c))
    }
  }
  for (let b = 0; b < n; b++) row[b] = quantise(row[b], set.quantum)
  return { row, loHz: set.loHz, hiHz: set.hiHz, source: set.source }
}

// ---------------------------------------------------------------------------------------------
// The wire, as the backend writes it.

/** One f32 the way serde_json writes it (ryu's shortest round-trip form, `1.0` for integers),
 *  so the payload sizes and parse costs measured here are the real ones. */
export function f32Json(v: number): string {
  const f = Math.fround(v)
  if (f === 0) return '0.0'
  for (let p = 1; p <= 9; p++) {
    const s = String(Number(f.toPrecision(p)))
    if (Math.fround(Number(s)) === f) return /[.e]/.test(s) ? s : `${s}.0`
  }
  return String(f)
}

/** One f64 span edge the way serde_json writes it. */
function spanJson(x: number): string {
  return Number.isInteger(x) ? `${x}.0` : String(x)
}

/** `Spectrum` serialised field for field as the backend sends it (row f32, span f64). */
export function frameJson(f: Frame): string {
  return `{"row":[${f.row.map(f32Json).join(',')}],"loHz":${spanJson(f.loHz)},"hiHz":${spanJson(f.hiHz)},"source":${JSON.stringify(f.source)}}`
}

/**
 * `SpectrumFrame` (`crates/tempo-app/src/dto.rs`) as the backend writes it, after its first two fields: `seq` and
 * `tMs` change with every answer and `frameReply` puts them in front, so the bins are still formatted once, before
 * the clock starts. The scale and slice are what each source's producer says: the audio FFT's exact dBFS axis, the
 * radio's own relative scale otherwise, and the Main receiver for the CI-V and FT-710 scopes.
 */
export function frameTail(f: Frame): string {
  const scale = f.source === 'audio' ? '{"kind":"dbfs","loDb":-120.0,"hiDb":0.0}' : '{"kind":"relative"}'
  const slice = f.source === 'civ' || f.source === 'yaesu' ? ',"slice":0' : ''
  return `"source":${JSON.stringify(f.source)},"loHz":${spanJson(f.loHz)},"hiHz":${spanJson(f.hiHz)},"scale":${scale}${slice},"bins":[${f.row.map(f32Json).join(',')}]}`
}

/** One answer to the frame command: a frame's number and time in front of its `frameTail`. */
export function frameReply(seq: number, tMs: number, tail: string): string {
  return `{"seq":${seq},"tMs":${tMs},${tail}`
}
