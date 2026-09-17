// THE GATE ON THE OPTIMISTIC DIAL — the safety case for showing a dial the radio has not reached.
//
// Remote's whole transmit surface is FT-sequencer arming (pinned in Rust, `operations/station.rs`),
// the privilege check lives in the station and judges the station's own CAT readback, and the only
// frequency a browser ever sends is a `radio.frequency` target the station validates and confirms
// by readback. None of that can be reached by a number the browser drew on a screen — PROVIDED the
// number stays a display value. This file is what keeps that "provided" true.
//
// Three checks, each paired with a control that MUST trip it, because a clean result from a check
// nobody has seen fail is not a result:
//
//   1. BEHAVIOUR — with a provisional dial standing, the station's snapshot and the station's own
//      operation state do not carry it. That is the property, stated about the objects themselves
//      rather than about the code that builds them.
//   2. CONTAINMENT — only the four files that are supposed to name it do, and nothing anywhere
//      feeds it into a `dialMhz`. A fifth reader is a change to the safety case and has to be a
//      deliberate edit to this list, not a quiet import.
//   3. IT IS NEVER LEFT STANDING — a value the station never confirms reverts to the station's
//      dial within PROVISIONAL_MS and the operator is told; a confirmed one goes quietly.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative, resolve, sep } from 'node:path'
import { scriptedStation } from './__fixtures__/scripted-station'
import type { AppSnapshot } from '../types'

type Station = ReturnType<typeof scriptedStation>
const open: Station[] = []
afterEach(() => { open.splice(0).forEach(s => s.close()) })

/** Every rendering of a dial a reader might store it as — Hz, MHz, and the printed MHz. */
const renderings = (hz: number) => [String(hz), String(hz / 1e6), (hz / 1e6).toFixed(4)]
/** Does `value`, serialised, carry this dial anywhere inside it? */
const carries = (value: unknown, hz: number) => {
  const text = JSON.stringify(value) ?? ''
  return renderings(hz).some(r => text.includes(r))
}

/** A live workspace reads the station's snapshot continuously; the stream only starts flowing on
 *  the first read, so warm it the same way before measuring anything against its cadence. */
async function warm(station: Station) {
  const first = station.application.invoke<AppSnapshot>('get_snapshot')
  await vi.advanceTimersByTimeAsync(700)
  return first
}

async function tuned(rttMs = 100, options: Parameters<typeof scriptedStation>[2] = {}) {
  const station = scriptedStation(rttMs, 4, options)
  open.push(station)
  await vi.advanceTimersByTimeAsync(rttMs + 1500)
  await warm(station)
  const context = station.operations.getSnapshot().state!.controls!.context
  // One notch of 1 kHz on the readout digits. Nothing is on the wire yet — the burst is still
  // inside its 120 ms coalescing window — so the instant the digits move is the instant the
  // claim below has to hold.
  expect(station.tuning.nudge(1000, { dialMhz: station.radio.dialMhz, sideband: 'USB', context })).toBe(true)
  const hz = station.tuning.getProvisionalHz()!
  expect(hz, 'the digits show the dial that was asked for').toBe(Math.round(station.radio.dialMhz * 1e6) + 1000)
  return { station, hz }
}

describe('a provisional dial is not in anything the station said', () => {
  it('is absent from the station\'s operation state, and from the whole view before anything is sent', async () => {
    const { station, hz } = await tuned()
    expect(station.wire.some(w => w.type === 'stationControl'), 'nothing has left the browser yet').toBe(false)
    expect(carries(station.operations.getSnapshot().state, hz), 'OperationState').toBe(false)
    // The strongest form of the claim, and the one the operator's screen is actually drawn from.
    expect(carries(station.operations.getSnapshot(), hz), 'the whole operation view').toBe(false)
  })

  it('stays out of the station\'s snapshot and state once the command IS out', async () => {
    // A SLOW RADIO (3 s from CAT command to readback), and that is not decoration: once the radio
    // has actually arrived, the station's own snapshot carries this number legitimately, and no
    // check on the number alone can tell that from a leak. The window where the question has an
    // answer is the one where the command is out and the radio has not moved yet, so the fixture
    // is chosen to make that window seconds long instead of 200 ms.
    const { station, hz } = await tuned(100, { catMs: 3000 })
    await vi.advanceTimersByTimeAsync(400)
    // `controlPending` now holds the command the STATION is executing, which of course names the
    // target: that is a record of something sent, not a browser display value, and it has always
    // been there. What must stay clean is everything a privilege, band, mode or TX computation
    // reads — the station's snapshot and the state the station itself sent.
    expect(station.operations.getSnapshot().controlPending, 'a command really is out').not.toBeNull()
    const snapshot = station.application.invoke<AppSnapshot>('get_snapshot')
    await vi.advanceTimersByTimeAsync(600)
    expect(station.radio.dialMhz, 'the radio has NOT arrived yet').toBe(14.2)
    expect(carries(await snapshot, hz), 'AppSnapshot').toBe(false)
    expect(carries(station.operations.getSnapshot().state, hz), 'OperationState').toBe(false)
  })

  it('control: the check can fail — the same test against an object that DOES carry it trips', async () => {
    const { hz } = await tuned()
    // Exactly what a wiring mistake would look like: the provisional written into a snapshot's
    // dial. If this comes back clean the two assertions above are proving nothing.
    expect(carries({ radio: { dialMhz: hz / 1e6, band: '20m' } }, hz)).toBe(true)
    expect(carries({ radio: { dialMhz: hz } }, hz)).toBe(true)
    expect(carries({ controls: { context: { radioId: 1 } }, revision: 4 }, hz)).toBe(false)
  })
})

// ── CONTAINMENT ───────────────────────────────────────────────────────────────────────────────
/** The provisional dial, by every name it goes by. */
const NAMES = /provisional/i
/** A `dialMhz` — a prop, a field or an assignment — fed from a provisional value. This is the
 *  wiring mistake the whole gate exists for: the moment the optimistic number becomes A DIAL, it
 *  is an input to everything that reads one. */
const AS_A_DIAL = /\bdialMhz\s*[:=]\s*\{?[^,;)\n]*provisional/i

/** The only files allowed to name it: the controller that owns it, the hook that reads it off the
 *  controller, the one call site that passes it, and the readout that draws it. A file appearing
 *  here that is not in this list is a widening of the safety case. */
const ALLOWED = [
  'remote-web/wheel-tuning.ts',
  'remote-web/wheel-tuning-context.ts',
  'components/CockpitHeader.tsx',
  'components/FrequencyReadout.tsx',
]

type Source = { path: string; text: string }
/** The scanner, over (path, text) pairs so the control below can run the same code on a violation
 *  it makes up rather than on a second implementation of it. */
function violations(sources: Source[]): string[] {
  const found: string[] = []
  for (const { path, text } of sources) {
    if (NAMES.test(text) && !ALLOWED.includes(path)) found.push(`${path}: names the provisional dial`)
    const line = text.split('\n').findIndex(l => AS_A_DIAL.test(l))
    if (line >= 0) found.push(`${path}:${line + 1}: feeds a provisional value into a dialMhz`)
  }
  return found
}

function sources(): Source[] {
  const root = resolve(process.cwd(), 'src')
  const out: Source[] = []
  const walk = (dir: string) => {
    for (const entry of readdirSync(dir)) {
      const p = join(dir, entry)
      if (statSync(p).isDirectory()) walk(p)
      else if (/\.tsx?$/.test(p) && !/\.test\.tsx?$/.test(p)) out.push({ path: relative(root, p).split(sep).join('/'), text: readFileSync(p, 'utf8') })
    }
  }
  walk(root)
  return out
}

describe('nothing reads the provisional dial but the digits', () => {
  it('only the four files that own, carry, pass and draw it name it at all', () => {
    const all = sources()
    expect(all.length, 'the tree was actually walked').toBeGreaterThan(200)
    expect(violations(all)).toEqual([])
    // ...and every file on the list really does still name it, so the list cannot rot into an
    // allowance for files that no longer exist.
    const naming = all.filter(s => NAMES.test(s.text)).map(s => s.path).sort()
    expect(naming).toEqual([...ALLOWED].sort())
  })

  it('control: a reader outside the list, and a provisional wired into a dial, both trip it', () => {
    expect(violations([{ path: 'stationAccess.ts', text: 'const dial = tuning.getProvisionalHz()\n' }]))
      .toEqual(['stationAccess.ts: names the provisional dial'])
    // The allowed files are not exempt from the second rule: this is the mistake that matters.
    expect(violations([{ path: 'components/CockpitHeader.tsx', text: 'a\nb\n<X dialMhz={remoteTuning.provisionalMhz} />\n' }]))
      .toEqual(['components/CockpitHeader.tsx:3: feeds a provisional value into a dialMhz'])
    expect(violations([{ path: 'remote-web/wheel-tuning.ts', text: 'snapshot.radio.dialMhz = this.provisional!.hz / 1e6\n' }]))
      .toEqual(['remote-web/wheel-tuning.ts:1: feeds a provisional value into a dialMhz'])
  })
})

// ── NEVER LEFT STANDING ───────────────────────────────────────────────────────────────────────
describe('a provisional value is never left standing', () => {
  it('reverts to the station\'s dial within 2.5 s of the gesture, and says it was not confirmed', async () => {
    // A station whose result read answers `pending` for ever: the command went out, the radio may
    // even have moved, but nothing ever comes back to say so. The digits must not keep the claim.
    const station = scriptedStation(100, 4, { pollSettles: false })
    open.push(station)
    await vi.advanceTimersByTimeAsync(1600)
    await warm(station)
    const context = station.operations.getSnapshot().state!.controls!.context
    station.tuning.nudge(1000, { dialMhz: station.radio.dialMhz, sideband: 'USB', context })
    await vi.advanceTimersByTimeAsync(2400)
    expect(station.tuning.getProvisionalHz(), 'still shown while it might yet be confirmed').not.toBeNull()
    expect(station.failures).toEqual([])
    await vi.advanceTimersByTimeAsync(200)
    expect(station.tuning.getProvisionalHz(), 'reverted to the station\'s dial').toBeNull()
    expect(station.failures, 'and said so').toEqual(['operationUnconfirmed'])
  })

  it('goes back at once when the station rejects it, rather than standing out the deadline', async () => {
    // The station answers, and the answer is not a readback. There is nothing to wait for: the
    // digits must not keep showing a dial the station has just said the radio is not on.
    const station = scriptedStation(100, 5, { eventOutcome: 'unknown' })
    open.push(station)
    await vi.advanceTimersByTimeAsync(1600)
    await warm(station)
    const context = station.operations.getSnapshot().state!.controls!.context
    station.tuning.nudge(1000, { dialMhz: station.radio.dialMhz, sideband: 'USB', context })
    expect(station.tuning.getProvisionalHz()).not.toBeNull()
    await vi.advanceTimersByTimeAsync(600)
    expect(station.tuning.getProvisionalHz(), 'reverted on the answer, not on the clock').toBeNull()
    expect(station.failures, 'and said once, not twice').toEqual(['operationUnconfirmed'])
    await vi.advanceTimersByTimeAsync(3000)
    expect(station.failures).toEqual(['operationUnconfirmed'])
  })

  it('a confirmed one goes quietly — the station\'s own reading carries the same number by then', async () => {
    const station = scriptedStation(100, 5)
    open.push(station)
    await vi.advanceTimersByTimeAsync(1600)
    await warm(station)
    const context = station.operations.getSnapshot().state!.controls!.context
    station.tuning.nudge(1000, { dialMhz: station.radio.dialMhz, sideband: 'USB', context })
    await vi.advanceTimersByTimeAsync(600)
    expect(station.radio.dialMhz, 'the radio is there').toBe(14.201)
    await vi.advanceTimersByTimeAsync(4000)
    expect(station.tuning.getProvisionalHz()).toBeNull()
    expect(station.failures, 'nothing to tell the operator about').toEqual([])
  })
})
