// @vitest-environment jsdom
//
// THE SCOPE'S FILTER EDGE AT ITS TWO HOSTS — what each cockpit hands PhoneScope, and what a width
// the edge reports becomes on the wire. PhoneScope is stubbed to a recorder of its props; the hook,
// the cockpit's gate and the write (`setFilterWidth`, the ± stepper's own) are real. By value:
// one write per flush, the last width, clamped to that cockpit's range — and none at all where the
// gate says the edge is not the operator's to move. And the Sub's marker each host hands it: only
// where the cockpit draws a SUB row, and only with a dial the snapshot knows.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { act, cleanup, render } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import { CwCockpit } from './CwCockpit'
import { setFilterWidth } from '../api'
import type { AppSnapshot } from '../types'

vi.mock('../api', async (importOriginal) => {
  // Every export auto-stubbed from the real module, so an API the cockpits gain cannot throw on
  // mount here; the reads below answer "nothing", so the cockpits settle quietly.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  return {
    ...auto,
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 } })),
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    askLog: vi.fn(() => new Promise(() => {})),
    lookupPark: vi.fn(async () => null),
    lookupParkLive: vi.fn(async () => null),
    qrzLookup: vi.fn(async () => null),
    resolveEntity: vi.fn(async () => null),
    searchParks: vi.fn(async () => []),
    cwDecode: vi.fn(async () => ({ text: '', wpm: 20, sent: [], candidates: [], state: 'listening' })),
    readRotator: vi.fn(async () => null),
    getDeclination: vi.fn(async () => null),
    getSatTrackStatus: vi.fn(async () => null),
    getSatTransponder: vi.fn(async () => null),
    setCwKeyer: vi.fn(async () => null),
    selectPeer: vi.fn(async () => null),
    setFilterWidth: vi.fn(async () => ({})),
  }
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (a: () => Promise<unknown>) => a()) }))
vi.mock('./CockpitHeader', () => ({
  CockpitHeader: ({ children }: { children?: unknown }) => <header className="cockpit-header">{children as never}</header>,
}))
vi.mock('./BandStrip', () => ({ BandStrip: () => null }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => null }))
vi.mock('./LogEntry', () => ({ LogEntry: () => null }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))
// The scope, as the host sees it: the props it was last handed.
type ScopeProps = {
  passbandHz?: number | null
  onPassband?: (hz: number) => void
  notchHz?: number | null
  subReceiver?: { dialHz: number; sideband: string; widthHz: number | null } | null
}
let scopeProps: ScopeProps = {}
vi.mock('./PhoneScope', () => ({
  PhoneScope: (p: ScopeProps) => {
    scopeProps = p
    return null
  },
}))

const write = vi.mocked(setFilterWidth)

function phoneSnap(radio: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    hunt: null,
    radio: {
      dialMhz: 14.25, band: '20m', catOk: true, sideband: 'USB', sidebandOverride: null, rigMode: 'USB',
      transmitting: false, txEnabled: true, txAllowed: true, qsoRecording: false, rfPower: null, micGain: null,
      nrLevel: 0.3, agc: 'fast', nb: true, nr: true, notch: null, comp: null, vox: null, filterWidthHz: 2400,
      splitTxMhz: null, smeterDb: null, rxLevel: 0, phoneSegLo: null, phoneSegHi: null, operatingMode: 'phone',
      ...radio,
    },
  } as unknown as AppSnapshot
}
function cwSnap(radio: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.05, band: '20m', catOk: true, sideband: 'USB', rigMode: 'CW', transmitting: false,
      txEnabled: true, txAllowed: true, cwWpm: 22, cwKeyer: 'cat', nrLevel: 0.3, agc: 'fast', nb: true,
      nr: true, notch: null, filterWidthHz: 500, splitTxMhz: null, smeterDb: null, operatingMode: 'cw',
      ...radio,
    },
    aiCw: { enabled: true, status: 'ready' },
  } as unknown as AppSnapshot
}

async function settle() {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}
const phone = async (radio?: Record<string, unknown>) => {
  render(<PhoneCockpit snap={phoneSnap(radio)} theme="dark" onSnap={() => {}} onConsumeWork={() => {}}
    pendingWork={null} fieldDay={undefined} wheelSensitivity={1} spots={[]} onWorkSpot={() => {}} />)
  await settle()
}
const cw = async (radio?: Record<string, unknown>) => {
  render(<CwCockpit snap={cwSnap(radio)} theme="dark" onWorkSpot={() => {}} spots={[]} />)
  await settle()
}
/** Report widths as an edge drag does, then let one flush pass. */
async function drag(...widths: number[]) {
  for (const w of widths) scopeProps.onPassband?.(w)
  await act(async () => {
    vi.advanceTimersByTime(120)
    await Promise.resolve()
  })
}

beforeEach(() => {
  vi.useFakeTimers({ toFake: ['setTimeout', 'clearTimeout'] })
  globalThis.ResizeObserver = class { observe() {} disconnect() {} unobserve() {} } as unknown as typeof ResizeObserver
  scopeProps = {}
  write.mockClear()
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

describe('Phone hands the scope a grabbable edge only where it is honest', () => {
  it('the rig reports 2.4 kHz in USB: the edge is live, and a drag writes ONE width, the last, clamped', async () => {
    await phone()
    expect(scopeProps.passbandHz).toBe(2400)
    expect(scopeProps.onPassband).toBeTypeOf('function')
    await drag(2000, 3000, 2449)
    expect(write.mock.calls).toEqual([[2400]])
    await drag(9000)
    expect(write.mock.calls).toEqual([[2400], [4000]])
  })

  it.each([
    ['the rig reports no width', { filterWidthHz: null }],
    ['FM, whose filter is fixed', { sidebandOverride: 'FM', rigMode: 'FM' }],
    ['a DATA mode, whose filter is FT8’s', { rigMode: 'PKTUSB' }],
    ['CAT is down', { catOk: false }],
  ])('%s: no edge to grab', async (_why, radio) => {
    await phone(radio)
    expect(scopeProps.onPassband).toBeUndefined()
  })

  it.each([
    ['Nexus is transmitting', { transmitting: true }],
    ['the rig is keyed at its own mic', { rigKeyed: true }],
  ])('%s: NO WRITE, whatever the edge reports', async (_why, radio) => {
    await phone(radio)
    await drag(1800)
    expect(write).not.toHaveBeenCalled()
  })

  it('passes the manual notch only while CAT reports it on', async () => {
    await phone({ manualNotch: true, notchFreqHz: 1200 })
    expect(scopeProps.notchHz).toBe(1200)
    cleanup()
    await phone({ manualNotch: false, notchFreqHz: 1200 })
    expect(scopeProps.notchHz).toBeNull()
  })
})

describe('CW hands the scope its own range, and nothing on the soundcard keyer', () => {
  it('a drag writes ONE width in CW’s 50 Hz steps, inside its 50–2000 Hz range', async () => {
    await cw()
    expect(scopeProps.passbandHz).toBe(500)
    await drag(700, 512)
    expect(write.mock.calls).toEqual([[500]])
    await drag(3000)
    expect(write.mock.calls).toEqual([[500], [2000]])
  })

  it('the soundcard keyer: no edge, and no CW-shaped passband over the DATA filter', async () => {
    await cw({ cwKeyer: 'soundcard', rigMode: 'PKTUSB' })
    expect(scopeProps.onPassband).toBeUndefined()
    expect(scopeProps.passbandHz).toBeNull()
  })
})

// An IC-7610's receivers on Nexus's own CI-V path: a Sub it offers and can command, whose dial the
// snapshot knows (today only an acknowledged split riding the Sub band carries one).
const OWN = { frontEnd: 'own', dsp: 'own', audio: 'own' }
const sub = (over: Record<string, unknown> = {}) => ({
  id: 'sub', stages: { frontEnd: 'own', dsp: 'unknown', audio: 'own' }, dialMhz: 14.23, band: '20m', sideband: 'USB', ...over,
})
const receivers = (over: Record<string, unknown> = {}) => ({
  main: { id: 'main', stages: OWN }, sub: sub(), subCapability: 'present', subCommandable: true, ...over,
})
const HOSTS = [
  ['Phone', phone],
  ['CW', cw],
] as const

describe('both hosts mark the Sub on the scope, where a SUB row is drawn and its dial is known', () => {
  it.each(HOSTS)("%s: the Sub's dial and sideband, and no width the Sub did not report", async (_host, mount) => {
    await mount({ receivers: receivers() })
    expect(scopeProps.subReceiver).toEqual({ dialHz: 14_230_000, sideband: 'USB', widthHz: null })
  })

  it.each(HOSTS)('%s: a width the Sub reports goes with it only beside the side it sits on', async (_host, mount) => {
    await mount({ receivers: receivers({ sub: sub({ filterWidthHz: 2400 }) }) })
    expect(scopeProps.subReceiver).toEqual({ dialHz: 14_230_000, sideband: 'USB', widthHz: 2400 })
    cleanup()
    await mount({ receivers: receivers({ sub: sub({ filterWidthHz: 2400, sideband: null }) }) })
    expect(scopeProps.subReceiver).toEqual({ dialHz: 14_230_000, sideband: '', widthHz: null })
  })

  const NONE: [string, Record<string, unknown>][] = [
    ['a station older than the receivers field', {}],
    ['a radio with one receiver', { receivers: receivers({ sub: null, subCapability: 'absent' }) }],
    ['a radio nobody has read a manual for', { receivers: receivers({ sub: null, subCapability: 'unknown' }) }],
    ['a Sub on a CAT path that cannot name it ("Hide it")', { receivers: receivers({ subCommandable: false }) }],
    ['a route the radio loop has not reported yet', { receivers: receivers({ subCommandable: null }) }],
    ["a Sub whose dial is not known", { receivers: receivers({ sub: sub({ dialMhz: null }) }) }],
  ]
  it.each(HOSTS.flatMap(([host, mount]) => NONE.map(([why, radio]) => [host, why, mount, radio] as const)))(
    '%s, %s: no Sub mark',
    async (_host, _why, mount, radio) => {
      await mount(radio)
      expect(scopeProps.subReceiver ?? null).toBeNull()
    },
  )
})
