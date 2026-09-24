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
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup, act } from '@testing-library/react'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { CwCockpit } from './CwCockpit'
import type { AppSnapshot, ReceiversStatus, ReceiverStatus } from '../types'

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
