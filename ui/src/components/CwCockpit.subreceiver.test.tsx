// @vitest-environment jsdom
//
// THE CW COCKPIT AND A SECOND RECEIVER (dual-receiver programme, the cockpit stage).
//
// ⛔ A RADIO WITH ONE RECEIVER IS DRAWN EXACTLY AS IT WAS. Pinned by a GOLDEN — the whole
// rendered cockpit, byte for byte, captured from the tree BEFORE CW learned about a Sub — never
// by a regex over the output. Every single-receiver shape the snapshot can carry must render
// that same document: no `receivers` at all (a station older than the field), and each of the
// three reasons a Sub is not offered (UNKNOWN, ABSENT, and a second receiver this build does not
// offer).
//
// ⚠️ jsdom NEVER LAYS OUT. The golden pins structure, text and attributes; geometry is the
// browser harness's job.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot, ReceiversStatus, ReceiverStatus } from '../types'
import type { CwPanelId, PanelLayoutApi } from '../features/panelState'
import { setAfGain, setRfGain, setSquelch, setNrLevel, setSubLevel } from '../api'

const decodeState = {
  text: 'CQ CQ DE KD9TAW',
  wpm: 22,
  sent: [] as string[],
  keyerError: null as string | null,
  candidates: [] as { call: string; best: boolean }[],
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null as string | null,
  workedCall: null as string | null,
  rst: null as string | null,
  name: null as string | null,
}

vi.mock('../api', async (importOriginal) => {
  // Every export auto-stubbed from the REAL module, so an export added later cannot make the
  // cockpit throw on mount; the few below return the shapes CW's mount actually reads.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    getLicensedBandPlan: vi.fn(async () => []),
    getBandPlan: vi.fn(async () => []),
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
    cwDecode: vi.fn(async () => decodeState),
    previewCw: vi.fn(async (t: string) => t),
    pointRotatorAtCall: vi.fn(async () => 0),
    // ⚠️ THE LIVE METER POLL IS A TIMER, so whether its first answer lands before the golden is
    // read depends on how loaded the box is. Answering with the bus's own resting reading makes
    // the document the same either way — an `{}` answer flipped the zero-beat indicator out of
    // `idle` (its tone read as `undefined`, not `null`) on a busy run only.
    getMeters: vi.fn(async () => ({ rxLevel: 0, smeterDb: null, cwToneHz: null })),
  }
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div data-testid="scope-stub" /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
})
afterEach(cleanup)

/** A rig that reports every receive control CW draws, so the whole rig strip is really drawn. */
const RIG = {
  dialMhz: 14.05,
  band: '20m',
  catOk: true,
  sideband: 'USB',
  rigMode: 'CW',
  transmitting: false,
  txEnabled: true,
  txAllowed: true,
  cwWpm: 22,
  cwKeyer: 'cat',
  nrLevel: 0.3,
  agc: 'fast',
  refusedAgc: null,
  nb: true,
  nr: true,
  notch: false,
  filterWidthHz: 500,
  splitTxMhz: null,
  smeterDb: null,
  rfGain: 1,
  afGain: 0.5,
  squelch: 0,
}

const MAIN: ReceiverStatus = { id: 'main', stages: { frontEnd: 'own', dsp: 'own', audio: 'own' }, dialMhz: 14.05 }

/** The snapshot with `receivers` exactly as given — `undefined` leaves the key off altogether,
 *  the shape a station older than the field sends. NO default: omitting it is a choice. */
function snapWith(receivers: ReceiversStatus | undefined): AppSnapshot {
  const radio: Record<string, unknown> = { ...RIG }
  if (receivers !== undefined) radio.receivers = receivers
  return { mycall: 'KD9TAW', mygrid: 'EN52', radio } as unknown as AppSnapshot
}

/** The rendered cockpit once its mount-time reads have settled, React's generated ids made
 *  position-independent (they count useId calls across the whole file). */
async function cockpitHtml(receivers: ReceiversStatus | undefined): Promise<string> {
  const { container } = render(
    <CwCockpit snap={snapWith(receivers)} theme="dark" onWorkSpot={() => {}} spots={[]} />,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  const html = container.innerHTML.replace(/:r[0-9a-z]+:/g, ':rID:')
  cleanup()
  return html
}

const GOLDEN = resolve(process.cwd(), 'src/components/__fixtures__/CwCockpit.single-receiver.golden.html')

describe('⛔ a radio with ONE receiver is drawn exactly as it was', () => {
  it('the cockpit matches the golden captured before CW learned about a Sub', async () => {
    expect(await cockpitHtml(undefined)).toBe(readFileSync(GOLDEN, 'utf8'))
  })

  it('every single-receiver shape of the snapshot draws that same document', async () => {
    const golden = readFileSync(GOLDEN, 'utf8')
    for (const subCapability of ['unknown', 'absent', 'present'] as const) {
      expect(await cockpitHtml({ main: MAIN, sub: null, subCapability, subCommandable: null }), subCapability).toBe(golden)
    }
  })

  // Regenerate ONLY from a tree whose single-receiver cockpit is known good — it DEFINES the
  // baseline: `NEXUS_REGEN_GOLDEN=1 vitest run src/components/CwCockpit.subreceiver.test.tsx`.
  it.skipIf(!process.env.NEXUS_REGEN_GOLDEN)('regenerate the golden', async () => {
    writeFileSync(GOLDEN, await cockpitHtml(undefined))
  })
})

// ── THE SUB ROW IN CW'S RIG STRIP (operator ruling 2026-09-23: "Phone and CW") ──────────────
//
// The same component Phone hosts, inside CW's one "Rig controls" frame, after Main's strip and
// gated on the RX DSP group's ⊞ id (`rxdsp`) — the Sub's levels are receive levels, so hiding
// the receive levels hides them too. No new ⊞ id, no stop control anywhere near it.

const SUB_7610: ReceiverStatus = { id: 'sub', stages: { frontEnd: 'own', dsp: 'unknown', audio: 'own' } }
/** A dual-receiver snapshot; `subCommandable` REQUIRED, never defaulted — it is under test. */
const dual = (sub: ReceiverStatus, subCommandable: boolean | null): ReceiversStatus => ({
  main: MAIN,
  sub,
  subCapability: 'present',
  subCommandable,
})

function fakePanels(removed: CwPanelId[]): PanelLayoutApi<CwPanelId> {
  return {
    layout: { v: 1, state: {}, share: {} },
    stateOf: (id: CwPanelId) => (removed.includes(id) ? 'removed' : 'docked'),
    setPanelState: () => {},
    shareOf: () => 1,
    setShare: () => {},
    setShares: () => {},
    undo: () => {},
    canUndo: false,
    undoRemoves: [],
    reset: () => {},
  } as unknown as PanelLayoutApi<CwPanelId>
}

async function mountDual(receivers: ReceiversStatus, removed: CwPanelId[] | null = null) {
  render(
    <CwCockpit
      snap={snapWith(receivers)}
      theme="dark"
      onWorkSpot={() => {}}
      spots={[]}
      {...(removed ? { panels: fakePanels(removed) } : {})}
    />,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

const subRow = () => document.querySelector('[data-receiver="sub"]')
const subRows = () => [...(subRow()?.querySelectorAll('[data-chain]') ?? [])].map((e) => e.getAttribute('data-chain'))

const mocks = [setAfGain, setRfGain, setSquelch, setNrLevel, setSubLevel].map((f) => f as unknown as ReturnType<typeof vi.fn>)
beforeEach(() => mocks.forEach((m) => m.mockClear()))

describe('CW: a confirmed, commandable Sub gets a SUB row in the rig strip', () => {
  it('in the Rig controls frame, after Main’s strip: RF · AF · SQL, and MAIN heads Main’s strip', async () => {
    await mountDual(dual(SUB_7610, true))
    expect(subRow(), 'no SUB row').not.toBeNull()
    expect(subRow()!.closest('[data-pane]')?.getAttribute('data-pane')).toBe('rigctl')
    expect(subRows()).toEqual(['RF', 'AF', 'SQL'])
    const strip = document.querySelector('[data-pane="rigctl"] .cw-rigctl')!
    const plate = strip.querySelector('[data-receiver-plate="main"]')
    expect(plate, 'no MAIN plate on Main’s strip').not.toBeNull()
    expect(plate!.textContent).toBe('MAIN')
    expect(strip.firstElementChild, 'MAIN heads Main’s strip').toBe(plate)
    // Main's strip comes first, the Sub's after it.
    expect(strip.compareDocumentPosition(subRow()!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy()
  })

  it('⛔ a Sub Nexus cannot command gets NO row — the cockpit is the single-receiver golden', async () => {
    const golden = readFileSync(GOLDEN, 'utf8')
    for (const subCommandable of [false, null] as const) {
      expect(await cockpitHtml(dual(SUB_7610, subCommandable)), `subCommandable=${subCommandable}`).toBe(golden)
    }
  })

  it('⭐ the Sub’s slider commands the Sub, and never one of Main’s setters', async () => {
    await mountDual(dual(SUB_7610, true))
    fireEvent.change(screen.getByLabelText('Sub receiver AF gain'), { target: { value: '40' } })
    expect(setSubLevel).toHaveBeenCalledWith('af', 0.4)
    for (const main of [setAfGain, setRfGain, setSquelch, setNrLevel]) {
      expect(main, 'the Sub’s AF reached one of Main’s setters').not.toHaveBeenCalled()
    }
  })

  it('hiding the RX DSP group (⊞ rxdsp) hides the SUB row with it, and nothing else does', async () => {
    await mountDual(dual(SUB_7610, true), ['rxdsp'])
    expect(subRow()).toBeNull()
    expect(document.querySelector('[data-receiver-plate="main"]'), 'no SUB row, no MAIN plate').toBeNull()
    cleanup()
    // CONTROL: hiding a different group leaves it.
    await mountDual(dual(SUB_7610, true), ['dsp'])
    expect(subRow()).not.toBeNull()
  })

  it('a browser that does not hold station control sees the row with every Sub slider dead', async () => {
    const { StationControlContext } = await import('../stationAccess')
    render(
      <StationControlContext.Provider value={false}>
        <CwCockpit snap={snapWith(dual(SUB_7610, true))} theme="dark" onWorkSpot={() => {}} spots={[]} />
      </StationControlContext.Provider>,
    )
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(subRow()).not.toBeNull()
    const inputs = [...subRow()!.querySelectorAll('input[type="range"]')] as HTMLInputElement[]
    expect(inputs.length).toBe(3)
    for (const i of inputs) expect(i.disabled, i.getAttribute('aria-label') ?? '').toBe(true)
    fireEvent.change(screen.getByLabelText('Sub receiver AF gain'), { target: { value: '40' } })
    expect(setSubLevel, 'a dead slider sent a Sub level').not.toHaveBeenCalled()
  })
})
