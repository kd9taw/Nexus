// #174 (barnburner6503): "filter the spots and alerts origins by country or continent … instead
// of having all world spots available". The Spots half shipped in 1.13.0 as the "Spotted from"
// chips; alerts had NO geographic gate of any kind — only the per-type BAND scopes, the three
// master switches and the watch list.
//
// The scope here is the operator's own: the entities they want to be INTERRUPTED about. Two
// things this file is built to prove, because both are how the feature could hurt someone:
//
//   1. AN OPERATOR WHO NEVER OPENS THE SETTING HEARS EXACTLY WHAT THEY HEARD BEFORE. That is the
//      first describe block, and it is the one protecting all ~1000 existing installs.
//   2. The two exemptions hold. A filter that can silence someone CALLING YOU, or a call the
//      operator explicitly typed into the watch list, is a bug however well it filters.
//
// Every scope below is built from entities on TWO continents. A fixture with one continent
// cannot tell "filtered correctly" from "not filtered at all".
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { __resetAlertsForTest, processDecodes } from './alerts'
import { scopedEntities } from './features/dxccGeo'
import { pushToast } from './toast'
import type { DecodeRow, Settings } from './types'

vi.mock('./toast', () => ({ pushToast: vi.fn() }))
const toasts = vi.mocked(pushToast)

// Node test env has no window; alerts.ts only needs setTimeout + (optional) AudioContext.
vi.stubGlobal('window', { setTimeout } as unknown as Window & typeof globalThis)

const settings = { alertMyCall: true, alertNew: true, alertCq: true } as unknown as Settings
const ctx = { state: 'Listening' as const, dxcall: null }

const TABLE: ReadonlyMap<string, string> = new Map([
  ['Fed. Rep. of Germany', 'EU'],
  ['France', 'EU'],
  ['Japan', 'AS'],
])

/** The operator's scope: Europe only. Japan is the discriminator — it must be refused while
 *  Germany is admitted, so a test cannot pass by filtering everything or nothing. */
const EUROPE_ONLY = scopedEntities(['EU'], [], TABLE)

let seq = 0
function decode(over: Partial<DecodeRow>): DecodeRow {
  return {
    from: 'F5XYZ',
    message: `msg-${seq++}`,
    freqHz: 1500 + seq,
    directedToMe: false,
    newDxcc: false,
    newGrid: false,
    isCq: false,
    ...over,
  } as unknown as DecodeRow
}

beforeEach(() => {
  toasts.mockClear()
  __resetAlertsForTest()
})

// ⭐ THE REGRESSION GUARD FOR EVERY EXISTING OPERATOR. The scope argument is what the app passes
// when the operator has configured nothing, and every alert kind must behave exactly as it did
// before this feature existed.
describe('an unconfigured scope changes nothing', () => {
  for (const [label, scope] of [
    ['absent (an older caller that passes no scope at all)', undefined],
    ['null (settings loaded, nothing ticked)', null],
  ] as const) {
    it(`alerts on a new DXCC from anywhere — scope ${label}`, () => {
      processDecodes(
        [decode({ from: 'JA1ABC', newDxcc: true, country: 'Japan' })],
        settings,
        undefined,
        ctx,
        undefined,
        14.074,
        scope,
      )
      expect(toasts).toHaveBeenCalledTimes(1)
      expect(toasts.mock.calls[0][0]).toContain('JA1ABC')
    })
  }

  it('alerts on a CQ from anywhere', () => {
    processDecodes(
      [decode({ from: 'JA1CQ', isCq: true, country: 'Japan' })],
      settings,
      undefined,
      ctx,
      undefined,
      14.074,
      null,
    )
    expect(toasts).toHaveBeenCalledTimes(1)
  })
})

describe('a configured scope gates the generic alert kinds', () => {
  it('refuses a new DXCC outside the scope and admits one inside it', () => {
    // Outside: Japan is not in Europe.
    processDecodes(
      [decode({ from: 'JA1ABC', newDxcc: true, country: 'Japan' })],
      settings,
      undefined,
      ctx,
      undefined,
      14.074,
      EUROPE_ONLY,
    )
    expect(toasts, 'Japan is outside Europe').not.toHaveBeenCalled()

    // Inside: the SAME alert kind, same band, same settings — only the entity differs. That is
    // what makes this a filter test rather than an "is anything on" test.
    processDecodes(
      [decode({ from: 'DL1ABC', newDxcc: true, country: 'Fed. Rep. of Germany' })],
      settings,
      undefined,
      ctx,
      undefined,
      14.074,
      EUROPE_ONLY,
    )
    expect(toasts, 'Germany is inside Europe').toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('DL1ABC')
  })

  it('refuses a CQ outside the scope and admits one inside it', () => {
    processDecodes(
      [
        decode({ from: 'JA1CQ', isCq: true, country: 'Japan' }),
        decode({ from: 'F5CQ', isCq: true, country: 'France' }),
      ],
      settings,
      undefined,
      ctx,
      undefined,
      14.074,
      EUROPE_ONLY,
    )
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('F5CQ')
  })

  it('refuses a new grid outside the scope', () => {
    processDecodes(
      [decode({ from: 'JA1GRID', newGrid: true, grid: 'PM95', country: 'Japan' })],
      settings,
      undefined,
      ctx,
      undefined,
      50.313,
      EUROPE_ONLY,
    )
    expect(toasts).not.toHaveBeenCalled()
  })
})

// The two things no geographic scope may ever silence. Both mirror `isHiddenByCountry`'s own
// protections — "someone calling US outranks any view filter" — because an operator who narrowed
// their alerts to Europe did not thereby ask to miss a JA station answering their CQ.
describe('the exemptions', () => {
  it('SOMEONE CALLING YOU alerts from outside the scope', () => {
    processDecodes(
      [decode({ from: 'JA1ME', directedToMe: true, country: 'Japan' })],
      settings,
      undefined,
      ctx,
      undefined,
      14.074,
      EUROPE_ONLY,
    )
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('JA1ME')
  })

  it('a WATCH-LIST hit alerts from outside the scope', () => {
    processDecodes(
      [decode({ from: 'JA1DX', isCq: true, country: 'Japan' })],
      settings,
      undefined,
      ctx,
      [{ id: 'w1', kind: 'call', value: 'JA1DX' }],
      14.074,
      EUROPE_ONLY,
    )
    expect(toasts).toHaveBeenCalledTimes(1)
    expect(toasts.mock.calls[0][0]).toContain('JA1DX')
  })

  it('a station cty.dat could not place still alerts — absence is not a match', () => {
    processDecodes(
      [decode({ from: 'XX9ZZ', newDxcc: true, country: undefined })],
      settings,
      undefined,
      ctx,
      undefined,
      14.074,
      EUROPE_ONLY,
    )
    expect(toasts).toHaveBeenCalledTimes(1)
  })
})

// A refused decode must not burn its dedup key: the operator can widen the scope mid-session,
// and the alert they then expect is the one that was refused a moment ago.
describe('a refused decode is not remembered', () => {
  it('alerts once the scope is widened to admit it', () => {
    const row = () => decode({ from: 'JA1ABC', newDxcc: true, country: 'Japan', message: 'CQ JA1ABC' })
    processDecodes([row()], settings, undefined, ctx, undefined, 14.074, EUROPE_ONLY)
    expect(toasts, 'refused while the scope was Europe').not.toHaveBeenCalled()

    processDecodes([row()], settings, undefined, ctx, undefined, 14.074, scopedEntities(['EU', 'AS'], [], TABLE))
    expect(toasts, 'and fires once Asia is added').toHaveBeenCalledTimes(1)
  })
})
