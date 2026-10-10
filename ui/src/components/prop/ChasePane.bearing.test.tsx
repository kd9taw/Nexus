// @vitest-environment jsdom
//
// THE CHASE BOX'S HEADING IS WHERE ITS ↗ TURNS THE ANTENNA. The ↗ is a point-at-call, which aims at the
// station's own grid or callbook position when Nexus knows one and at the centre of its country only when
// it knows none; the heading beside it was always the centre of the country, worked out in the page. EC1DD
// from JO21EV is the tester's case: its grid IN52TK is at 227°, the centre of Spain at about 208°, so the
// number a row draws says which of the two it took. The heading is now the station's answer
// (`rotatorBearingToCall`, the point's own resolver read only; that it equals what the point turns to is
// proven by value in src-tauri, `the_bearing_the_box_shows_is_the_one_point_turns_the_antenna_to`), and
// this file holds the box to drawing exactly that answer for each row's own call.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, render, waitFor } from '@testing-library/react'
import type { CallBearing, NeedAlert } from '../../types'
import type { PaneContext } from '../connect/paneContext'
import { StationControlContext } from '../../stationAccess'
import { t } from '../../i18n'
import { ChasePane } from './ChasePane'

vi.setConfig({ testTimeout: 15_000 })

// The one command the box may reach for its headings.
const api = vi.hoisted(() => ({
  rotatorBearingToCall: vi.fn((_call: string): Promise<CallBearing> => Promise.reject(new Error('unset'))),
}))
vi.mock('../../api', () => api)

/** EC1DD from JO21EV: its own grid IN52TK, 227.35° and 1422.5 km away. */
const EC1DD: CallBearing = { pointed: { bearing: 227.35, to: 'grid', grid: 'IN52TK', country: null }, km: 1422.5 }
/** EA8ZZ: nothing known of the station, so the centre of the Canary Islands, 220.43° and 3112.2 km away. */
const EA8ZZ: CallBearing = { pointed: { bearing: 220.43, to: 'country', grid: null, country: 'Canary Is.' }, km: 3112.2 }
const answers: Record<string, CallBearing> = { EC1DD, EA8ZZ }

function need(call: string, entity: string, band = '20m'): NeedAlert {
  return {
    call, entity, band, zone: 14, tags: ['NewEntity'], priority: 90, headline: `${call} needed`, mode: 'CW',
    freqMhz: 14.025, admittedAt: Math.floor(Date.now() / 1000) - 120,
  } as unknown as NeedAlert
}

/** The box's context on the desktop. The centroids are where a heading worked out in the page points:
 *  the centre of Spain, about 209° from JO21EV; and the Canaries put at 0° N 0° E, nowhere near the
 *  station's own centre for them, so a number the page drew from either is told apart from the station's. */
function ctx(...alerts: NeedAlert[]): PaneContext {
  return {
    myGrid: 'JO21EV',
    entityCentroids: new Map([
      ['Spain', { lat: 40.4, lon: -3.7 }],
      ['Canary Is.', { lat: 0, lon: 0 }],
    ]),
    needAlerts: alerts,
    bandOutlook: null,
    prop: null,
    dxpedWindows: new Map(),
    onSelectCall: () => {},
    onPoint: () => {},
  } as unknown as PaneContext
}

const rowsOf = (call: string) =>
  [...document.querySelectorAll<HTMLElement>('.chase-row')].filter((row) => row.querySelector('.chase-call')?.textContent === call)
/** The heading drawn on `call`'s row(s), each one. */
const headings = (call: string) => rowsOf(call).map((row) => row.querySelector<HTMLElement>('.chase-az'))
/** The words a row says in place of a heading. */
const why = (call: string) => rowsOf(call)[0]?.querySelector<HTMLElement>('.chase-why') ?? null

beforeEach(() => {
  vi.clearAllMocks()
  api.rotatorBearingToCall.mockImplementation((call: string) =>
    answers[call] ? Promise.resolve(answers[call]) : Promise.reject(new Error('unknownStation')),
  )
})
afterEach(cleanup)

describe('the Chase box draws the bearing its ↗ turns to', () => {
  it("draws the station's bearing to EC1DD's own grid, not the centre of Spain", async () => {
    render(<ChasePane ctx={ctx(need('EC1DD', 'Spain'))} />)
    await waitFor(() => expect(headings('EC1DD')[0]?.textContent).toBe('227°'))
    expect(headings('EC1DD')[0]!.getAttribute('title')).toBe('227° true, short path')
    expect(api.rotatorBearingToCall).toHaveBeenCalledWith('EC1DD')
  })

  it("marks the station's own answer when it is only the centre of the country", async () => {
    render(<ChasePane ctx={ctx(need('EA8ZZ', 'Canary Is.'))} />)
    await waitFor(() => expect(headings('EA8ZZ')[0]?.textContent).toBe('~220°'))
    expect(headings('EA8ZZ')[0]!.getAttribute('title')).toContain('to the centre of Canary Is., not to this station')
  })

  it("gives each row its own call's answer, once for a call on two bands", async () => {
    render(<ChasePane ctx={ctx(need('EA8ZZ', 'Canary Is.'), need('EC1DD', 'Spain'), need('EC1DD', 'Spain', '15m'))} />)
    await waitFor(() => expect(headings('EC1DD').map((h) => h?.textContent)).toEqual(['227°', '227°']))
    expect(headings('EA8ZZ')[0]?.textContent).toBe('~220°')
    // One question per call however many rows it has: as often for EC1DD as for EA8ZZ's one row.
    const asked = (call: string) => api.rotatorBearingToCall.mock.calls.filter(([c]) => c === call).length
    expect(asked('EC1DD')).toBe(asked('EA8ZZ'))
  })

  it('says so in words when the station cannot place the call or has no grid of yours, never a 0°', async () => {
    render(<ChasePane ctx={ctx(need('QQ1QQ', 'Spain'))} />)
    await waitFor(() => expect(why('QQ1QQ')?.textContent).toBe('location unknown'))
    expect(headings('QQ1QQ')[0], 'no heading drawn beside the entity').toBeNull()
    cleanup()
    api.rotatorBearingToCall.mockImplementation(() => Promise.reject(new Error('noGrid')))
    render(<ChasePane ctx={ctx(need('EC1DD', 'Spain'))} />)
    await waitFor(() => expect(why('EC1DD')?.textContent).toBe(t('rotor.pane.aim.noGrid')))
    expect(headings('EC1DD')[0]).toBeNull()
    expect(document.body.textContent).not.toMatch(/\b0°/)
  })

  it('in a browser asks the station nothing and draws no heading it would not turn to', async () => {
    render(
      <StationControlContext.Provider value={false}>
        <ChasePane ctx={ctx(need('EC1DD', 'Spain'))} />
      </StationControlContext.Provider>,
    )
    // Control: the row is drawn.
    expect(headings('EC1DD')).toHaveLength(1)
    await new Promise((r) => setTimeout(r, 20))
    expect(headings('EC1DD')[0]).toBeNull()
    expect(why('EC1DD')).toBeNull()
    expect(api.rotatorBearingToCall).not.toHaveBeenCalled()
  })
})
