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
import { describe, it, expect, afterEach, beforeAll, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot, ReceiversStatus, ReceiverStatus } from '../types'

vi.mock('../api', async original => {
  const actual = await original<Record<string, unknown>>()
  const reads: Record<string, unknown> = { getLicensedBandPlan: [], getBandPlan: [], getCatCwUnprovenRigModels: [] }
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
