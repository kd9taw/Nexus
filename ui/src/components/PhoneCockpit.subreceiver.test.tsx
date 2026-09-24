// @vitest-environment jsdom
//
// THE PHONE COCKPIT AND A SECOND RECEIVER (dual-receiver programme, the cockpit stage).
//
// ⛔ A RADIO WITH ONE RECEIVER IS DRAWN EXACTLY AS IT WAS. That is pinned here by a GOLDEN — the
// whole rendered cockpit, byte for byte, captured from the tree BEFORE the Sub strip existed —
// never by a regex over the output, which passes whatever else changed. Every single-receiver
// shape the snapshot can carry must render that same document: no `receivers` at all (a
// station older than the field), and each of the three reasons a Sub is not offered (UNKNOWN,
// ABSENT, and a second receiver this build does not offer).
//
// ⚠️ jsdom NEVER LAYS OUT. The golden pins structure, text and attributes; geometry is the
// browser harness's job.
import { describe, it, expect, afterEach, beforeAll, beforeEach, vi } from 'vitest'
import { render, cleanup, fireEvent, screen } from '@testing-library/react'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot, ReceiversStatus, ReceiverStatus } from '../types'
import { setAfGain, setSubLevel } from '../api'
import { StationControlContext } from '../stationAccess'

vi.mock('../api', async original => {
  const actual = await original<Record<string, unknown>>()
  // `getMeters` answers the meter bus's own resting reading: the live meter poll is a timer, and
  // an `{}` answer landing (or not) before the golden is read would make the document depend on
  // how loaded the box is.
  const reads: Record<string, unknown> = {
    getLicensedBandPlan: [],
    getBandPlan: [],
    getCatCwUnprovenRigModels: [],
    getMeters: { rxLevel: 0, smeterDb: null, cwToneHz: null },
  }
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [name,
    typeof value === 'function' ? vi.fn(async () => structuredClone(reads[name] ?? {})) : value]))
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(cleanup)

/** An IC-7610's worth of Main readings, so every receive and transmit row is really drawn. */
const RIG = {
  dialMhz: 14.2,
  band: '20m',
  sideband: 'USB',
  catOk: true,
  rigMode: 'USB',
  transmitting: false,
  txEnabled: false,
  txAllowed: true,
  slot: 0,
  nextSlotMs: 0,
  filterWidthHz: 2400,
  attStepsDb: [6, 12, 18],
  attDb: 0,
  preampStepsDb: [1, 2],
  preampDb: 0,
  monitorGain: 0.4,
  rfGain: 1,
  afGain: 0.5,
  squelch: 0,
  micGain: 0.5,
  nrLevel: 0.3,
  notchFreqHz: 1500,
  compLevel: 0.35,
  agc: 'fast',
  nb: false,
  nr: false,
  notch: false,
  manualNotch: false,
  comp: true,
  vox: false,
}

const MAIN: ReceiverStatus = { id: 'main', stages: { frontEnd: 'own', dsp: 'own', audio: 'own' }, dialMhz: 14.2 }

/** The snapshot with `receivers` exactly as given — `undefined` leaves the key off altogether,
 *  the shape a station older than the field sends. NO default: omitting it is a choice. */
function snapWith(receivers: ReceiversStatus | undefined): AppSnapshot {
  const radio: Record<string, unknown> = { ...RIG }
  if (receivers !== undefined) radio.receivers = receivers
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    radio,
    link: {},
    qso: {},
    stations: [],
    conversations: [],
  } as unknown as AppSnapshot
}

/** The rendered cockpit, with React's generated ids (`:r3:`) made position-independent — they
 *  count useId calls across the whole file, so they differ between renders of the same tree. */
function cockpitHtml(receivers: ReceiversStatus | undefined): string {
  const { container } = render(<PhoneCockpit snap={snapWith(receivers)} theme="dark" />)
  const html = container.innerHTML.replace(/:r[0-9a-z]+:/g, ':rID:')
  cleanup()
  return html
}

const GOLDEN = resolve(process.cwd(), 'src/components/__fixtures__/PhoneCockpit.single-receiver.golden.html')

describe('⛔ a radio with ONE receiver is drawn exactly as it was', () => {
  it('the cockpit matches the golden captured before the Sub strip existed', () => {
    expect(cockpitHtml(undefined)).toBe(readFileSync(GOLDEN, 'utf8'))
  })

  it('every single-receiver shape of the snapshot draws that same document', () => {
    const golden = readFileSync(GOLDEN, 'utf8')
    for (const subCapability of ['unknown', 'absent', 'present'] as const) {
      expect(cockpitHtml({ main: MAIN, sub: null, subCapability, subCommandable: null }), subCapability).toBe(golden)
    }
  })

  // Regenerate ONLY from a tree whose single-receiver cockpit is known good — it DEFINES the
  // baseline: `NEXUS_REGEN_GOLDEN=1 vitest run src/components/PhoneCockpit.subreceiver.test.tsx`.
  it.skipIf(!process.env.NEXUS_REGEN_GOLDEN)('regenerate the golden', () => {
    writeFileSync(GOLDEN, cockpitHtml(undefined))
  })
})

// ── THE SUB STRIP — a confirmed dual receiver ──────────────────────────────────────────────
//
// Where it sits is the conservative additive choice (the dual-receiver ruling D9 names no host
// for the Sub): inside Phone's own RECEIVER pane, below Main's chain, drawn only when the
// snapshot offers a Sub. A new component, not a widened shared one — no meter or scope host
// changes (`SMeter`, `TxMeters`, `PhoneScope` are untouched).

/** The IC-7610's Sub by the vendor table: its own front end and AF, no documented DSP. */
const SUB_7610: ReceiverStatus = { id: 'sub', stages: { frontEnd: 'own', dsp: 'unknown', audio: 'own' } }
/** The IC-9700's Sub: its own front end; neither DSP nor AF documented per receiver. */
const SUB_9700: ReceiverStatus = { id: 'sub', stages: { frontEnd: 'own', dsp: 'unknown', audio: 'unknown' } }

/** A dual-receiver snapshot. `subCommandable` is REQUIRED — the route is under test in half
 *  of these, and a defaulted one would quietly make every case the routed one. */
const dual = (sub: ReceiverStatus, subCommandable: boolean | null): ReceiversStatus => ({
  main: MAIN,
  sub,
  subCapability: 'present',
  subCommandable,
})

function mountDual(receivers: ReceiversStatus, over: Record<string, unknown> = {}) {
  const snap = snapWith(receivers)
  Object.assign(snap.radio, over)
  return render(<PhoneCockpit snap={snap} theme="dark" />)
}

const subStrip = () => document.querySelector('[data-receiver="sub"]')
const subRows = () => [...(subStrip()?.querySelectorAll('[data-chain]') ?? [])].map((e) => e.getAttribute('data-chain'))
/** Main's chain rows — the receiver pane's, less the Sub strip's. */
const mainRxRows = () =>
  [...document.querySelectorAll('[data-pane="receiver"] [data-chain]')]
    .filter((e) => !e.closest('[data-receiver="sub"]'))
    .map((e) => e.getAttribute('data-chain'))

const mockSetSubLevel = setSubLevel as unknown as ReturnType<typeof vi.fn>
const mockSetAfGain = setAfGain as unknown as ReturnType<typeof vi.fn>
beforeEach(() => {
  mockSetSubLevel.mockClear()
  mockSetAfGain.mockClear()
})

describe('a confirmed dual receiver gets a Sub strip — and only one', () => {
  it('on a route that names the Sub: the strip, in the RECEIVER pane, RF · AF · SQL in signal order', () => {
    mountDual(dual(SUB_7610, true))
    expect(subStrip(), 'no Sub strip').not.toBeNull()
    expect(subStrip()!.closest('[data-pane]')?.getAttribute('data-pane')).toBe('receiver')
    expect(subRows()).toEqual(['RF', 'AF', 'SQL'])
    // Main's chain is Main's, whole and in its own order — the Sub strip is additive.
    expect(mainRxRows()).toEqual(['BW', 'ATT', 'PRE', 'RF', 'NB', 'NR', 'NRLVL', 'ANF', 'MN', 'NOTCHF', 'AGC', 'AF', 'SQL'])
  })

  it('⭐ D7: the IC-9700’s Sub offers RF alone, and names AF · SQL NOT CONFIRMED — never "not on this radio"', () => {
    mountDual(dual(SUB_9700, true))
    expect(subRows()).toEqual(['RF'])
    expect(subStrip()!.textContent).toContain('Not confirmed for the sub receiver: AF · SQL')
    expect(document.body.textContent, 'the Sub’s unknowns must not read as the radio lacking them').not.toMatch(/Not on this radio/)
  })

  it('⛔ a Sub Nexus cannot command gets NO row — the cockpit is the single-receiver golden, byte for byte', () => {
    // Operator ruling (2026-09-23, "Hide it"): an FTDX101, TS-990S, IC-9100, IC-910H or FTDX5000
    // on the Hamlib path — or an Icom run through Hamlib — shows no Sub row at all, and nothing
    // on its screen changes. The route not yet reported (the first moment after a connect) is
    // the same: no row until the radio loop says the Sub can be reached.
    const golden = readFileSync(GOLDEN, 'utf8')
    for (const subCommandable of [false, null] as const) {
      const html = cockpitHtml(dual(SUB_7610, subCommandable))
      expect(html, `subCommandable=${subCommandable}`).toBe(golden)
    }
  })

  it('no CAT: the Sub rows stay, dead', () => {
    mountDual(dual(SUB_7610, true), { catOk: false })
    const inputs = [...subStrip()!.querySelectorAll('input[type="range"]')] as HTMLInputElement[]
    expect(inputs.length).toBe(3)
    for (const i of inputs) expect(i.disabled, i.getAttribute('aria-label') ?? '').toBe(true)
  })

  it('⭐ A SUB CONTROL COMMANDS THE SUB, AND NEVER MAIN — and Main’s own control still commands Main', () => {
    mountDual(dual(SUB_7610, true))
    fireEvent.change(screen.getByLabelText('Sub receiver AF gain'), { target: { value: '40' } })
    expect(mockSetSubLevel).toHaveBeenCalledWith('af', 0.4)
    expect(mockSetAfGain, 'the Sub’s AF reached Main’s setter').not.toHaveBeenCalled()
    // CONTROL: Main's AF slider, same pane, same plate — Main's setter, not the Sub's.
    mockSetSubLevel.mockClear()
    fireEvent.change(screen.getByLabelText('AF gain'), { target: { value: '30' } })
    expect(mockSetAfGain).toHaveBeenCalledWith(0.3)
    expect(mockSetSubLevel, 'Main’s AF reached the Sub').not.toHaveBeenCalled()
  })

  it('the slider shows what the radio ACCEPTED; a level never set reads as unknown, not zero', () => {
    mountDual(dual({ ...SUB_7610, afGain: 0.25 }, true))
    const row = (id: string) => subStrip()!.querySelector(`[data-chain="${id}"]`)!
    expect(row('AF').textContent).toContain('25%')
    expect(row('RF').textContent).toContain('—')
    expect(row('RF').textContent).not.toContain('%')
  })

  it('the Sub’s dial where the engine knows it, and "not read" where it does not', () => {
    mountDual(dual({ ...SUB_9700, dialMhz: 145.965, band: '2m', sideband: 'LSB' }, true))
    expect(subStrip()!.textContent).toContain('145.9650')
    expect(subStrip()!.textContent).toContain('2m')
    expect(subStrip()!.textContent).toContain('LSB')
    cleanup()
    mountDual(dual(SUB_9700, true))
    expect(subStrip()!.textContent).not.toContain('145.9650')
    expect(subStrip()!.querySelector('[data-sub-dial]')?.textContent).toBe('—')
  })

  it('MAIN labels Main’s chain exactly while a SUB row is shown', () => {
    mountDual(dual(SUB_7610, true))
    const mainChain = document.querySelector('[data-pane="receiver"] .ph-chain:not(.ph-subrx)')!
    const plate = mainChain.querySelector('[data-receiver-plate="main"]')
    expect(plate, 'no MAIN plate on Main’s chain beside a SUB row').not.toBeNull()
    expect(plate!.textContent).toBe('MAIN')
    expect(mainChain.firstElementChild, 'MAIN heads the chain').toBe(plate)
    expect(document.querySelectorAll('[data-receiver-plate="main"]').length, 'one MAIN plate').toBe(1)
    // …and with no SUB row there is no MAIN plate: the golden cases above prove the whole
    // document; this is the direct statement.
    cleanup()
    mountDual(dual(SUB_7610, false))
    expect(document.querySelector('[data-receiver-plate="main"]')).toBeNull()
  })

  it('a browser that does not hold station control sees the row with every Sub slider dead', () => {
    // The Remote page draws the same row (SubReceiverRemote.test.tsx drives its real operation
    // contract); without control of a station that takes `radio.subLevel`, nothing is live.
    const snap = snapWith(dual(SUB_7610, true))
    render(
      <StationControlContext.Provider value={false}>
        <PhoneCockpit snap={snap} theme="dark" />
      </StationControlContext.Provider>,
    )
    expect(subStrip()).not.toBeNull()
    const inputs = [...subStrip()!.querySelectorAll('input[type="range"]')] as HTMLInputElement[]
    expect(inputs.length).toBe(3)
    for (const i of inputs) expect(i.disabled, i.getAttribute('aria-label') ?? '').toBe(true)
    fireEvent.change(screen.getByLabelText('Sub receiver AF gain'), { target: { value: '40' } })
    expect(mockSetSubLevel, 'a dead slider sent a Sub level').not.toHaveBeenCalled()
  })
})
