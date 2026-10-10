// @vitest-environment jsdom
//
// THE ROTOR BOX BESIDE EACH COCKPIT, IN THE REAL APP: its line 2 is the call in THAT cockpit's log
// entry (the call its Log action would write), with the station's bearing and distance to it, and
// Point turns the antenna there and does nothing else.
//   · FT's entry is the QSO's (Log QSO logs it), not the roster's selection, which FT's header strip
//     follows: the two are given different calls here, so the test can tell which one won.
//   · Phone, CW, RTTY, PSK and JS8: the call typed into the cockpit's log strip, gone when it is cleared.
//   · SSTV and APRS have no log entry: no line 2.
//   · The hosted Remote page draws neither the rail nor a box, so it gains nothing.
// The bearing shown being the one Point turns to is proven by value in src-tauri
// (`the_bearing_the_box_shows_is_the_one_point_turns_the_antenna_to`).
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { Mock } from 'vitest'

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers } = await import('./appCockpits.testkit')
  return { ...auto, ...appApiAnswers() }
})
vi.mock('./toast', async (importOriginal) => ({
  ...(await importOriginal<Record<string, unknown>>()),
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
vi.mock('./components/Waterfall', () => ({ Waterfall: () => <div data-testid="waterfall" /> }))
vi.mock('./components/MapView', () => ({ MapView: () => <div data-testid="map" /> }))

import * as api from './api'
import App from './App'
import { StationControlContext } from './stationAccess'
import { APP_SNAPSHOT, COCKPIT_MAIN } from './appCockpits.testkit'
import settingsFixture from './components/__fixtures__/defaultSettings.json'
import type { AppSnapshot, CallBearing, Settings } from './types'
import { t } from './i18n'
import { fmtDistanceKm, resolveUnits, UNITS_KEY } from './units'

// Each case mounts the real App; under the full suite's load that outruns vitest's default 5 s.
vi.setConfig({ testTimeout: 30_000 })

const SECTIONS_ON = { phone: true, cw: true, rtty: true, psk: true, sstv: true, aprs: true, js8: true, connect: true }
/** The rail with the Rotor box at its top. */
const RAIL_SLOTS = { rail1: 'rotor', rail2: 'clock', rail3: 'spacewx', rail4: 'getout' }

/** What the station answers for each call: EC1DD at its callbook grid, AA1AA at its log form's. */
const BEARINGS: Record<string, CallBearing> = {
  EC1DD: { pointed: { bearing: 227.4, to: 'grid', grid: 'IN52TK', country: null }, km: 1530.6 },
  AA1AA: { pointed: { bearing: 290.6, to: 'grid', grid: 'FN42KH', country: null }, km: 5379.2 },
}

/** FT's recall card follows the roster's selection and asks the station for that call's bearing too
 *  (features/callBearing), so a case that counts the box's questions puts the card away first. */
function withoutFtRecallCard(): void {
  localStorage.setItem('nexus.panels.operate.main', JSON.stringify({ v: 2, state: { recall: 'removed' }, share: {} }))
}

async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await waitFor(() => expect(document.documentElement.getAttribute('data-viewport')).not.toBeNull())
  await act(async () => {})
}

/** Line 2 for a call, in the unit this window resolved (App mirrors the setting, 'auto' by default). */
const line2For = (call: string) => {
  const b = BEARINGS[call]
  return `→ ${call} ${Math.round(b.pointed.bearing)}° (${fmtDistanceKm(b.km, resolveUnits(localStorage.getItem(UNITS_KEY)))})`
}
/** Every command that answers with a snapshot answers with this one, so the QSO a case sets stays set. */
function serve(snapshot: unknown): void {
  for (const answer of [api.getSnapshot, api.setOperatingMode, api.setArea, api.selectPeer]) {
    vi.mocked(answer as Mock).mockImplementation(async () => snapshot)
  }
}
const rail = () => document.querySelector<HTMLElement>('.dash-rail')
/** The rail's Rotor box line 2, or null. */
const railLine2 = () => rail()?.querySelector<HTMLElement>('.rotor-aim') ?? null
const railLine2Text = () => railLine2()?.querySelector('.rotor-aim-to')?.textContent ?? null
/** Every api mock and how often it has been called. */
const callCounts = () =>
  Object.fromEntries(
    Object.entries(api)
      .filter(([, v]) => vi.isMockFunction(v))
      .map(([k, v]) => [k, (v as Mock).mock.calls.length]),
  )

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: SECTIONS_ON }))
  localStorage.setItem('nexus.dashrail.config', JSON.stringify({ slots: RAIL_SLOTS }))
  document.documentElement.removeAttribute('data-viewport')
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  Object.defineProperty(window, 'innerHeight', { value: 1080, configurable: true })
  serve(APP_SNAPSHOT)
  // A rotator that reports 100°: the header strips draw their slew, and the box its whole surface.
  vi.mocked(api.readRotator).mockImplementation(async () => 100)
  vi.mocked(api.readRotatorState).mockImplementation(async () => ({ azDeg: 100, reading: 'position', elDeg: null, elRange: null }))
  vi.mocked(api.rotatorBearingToCall).mockReset()
  vi.mocked(api.rotatorBearingToCall).mockImplementation(async (call: string) => {
    if (BEARINGS[call]) return BEARINGS[call]
    throw 'unknownStation'
  })
  vi.mocked(api.pointRotatorAtCall).mockReset()
  vi.mocked(api.pointRotatorAtCall).mockImplementation(async (call: string) => BEARINGS[call].pointed)
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})

describe('FT: the box follows the QSO its Log QSO would log, not the roster selection the header strip follows', () => {
  /** The roster has AA1AA selected; the QSO in progress is with EC1DD. */
  const ftSnapshot = {
    ...APP_SNAPSHOT,
    activePeer: 'AA1AA',
    qso: { state: 'AwaitReport', dxcall: 'EC1DD', rxReport: null, running: false, cqRunning: false },
  }

  it('line 2 is EC1DD at its bearing, Point turns to EC1DD, and the strip still offers AA1AA', async () => {
    serve(ftSnapshot)
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ operate: true }))
    withoutFtRecallCard()
    await mountOn('operate')
    const ft = document.querySelector<HTMLElement>(COCKPIT_MAIN.operate)
    expect(ft, 'the FT cockpit did not render').not.toBeNull()
    expect(document.querySelector('.view-crash')).toBeNull()
    await waitFor(() => expect(railLine2Text()).toBe(line2For('EC1DD')), { timeout: 5000 })
    expect(api.rotatorBearingToCall).not.toHaveBeenCalledWith('AA1AA')
    // The control: FT's header strip is unchanged, and follows the roster's selection.
    await waitFor(() => expect(within(ft!).queryByRole('button', { name: '→ AA1AA' })).not.toBeNull())
    expect(within(ft!).queryByRole('button', { name: '→ EC1DD' })).toBeNull()

    // Point: the point-at-call for the ENTRY's call, short path, and no other command of any kind.
    const point = within(rail()!).getByRole('button', { name: t('rotor.pane.aim.point.label') })
    const before = callCounts()
    fireEvent.click(point)
    const after = callCounts()
    expect(Object.keys(after).filter((k) => after[k] !== before[k])).toEqual(['pointRotatorAtCall'])
    expect(api.pointRotatorAtCall).toHaveBeenCalledWith('EC1DD', false)
    expect(api.pointRotatorAtCall).not.toHaveBeenCalledWith('AA1AA', expect.anything())
  })

  it('a box standing in FT\'s own columns reads the same entry', async () => {
    serve(ftSnapshot)
    localStorage.setItem(
      'nexus.panels.operate.main',
      JSON.stringify({ v: 2, state: { box1: 'docked' }, share: {}, boxes: { box1: 'rotor' } }),
    )
    await mountOn('operate')
    const box = () => document.querySelector<HTMLElement>('main.operate-cockpit .pane-frame[data-pane="box1"]')
    await waitFor(() => expect(box()?.querySelector('.rotor-aim-to')?.textContent).toBe(line2For('EC1DD')), {
      timeout: 5000,
    })
  })

  it('no QSO, no line 2', async () => {
    serve({ ...ftSnapshot, qso: null })
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ operate: true }))
    withoutFtRecallCard()
    await mountOn('operate')
    await waitFor(() => expect(rail()?.querySelector('.rotor-pane')).toBeTruthy())
    await waitFor(() => expect(rail()!.querySelector('.rotor-az')?.textContent).toContain('100°T'))
    expect(railLine2()).toBeNull()
    expect(api.rotatorBearingToCall).not.toHaveBeenCalled()
  })
})

describe('the cockpits with a log strip: the call typed into it', () => {
  it.each(['phone', 'cw', 'rtty', 'psk', 'js8'].map((s) => [s]))('%s', async (section) => {
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ [section]: true }))
    await mountOn(section)
    const cockpit = document.querySelector<HTMLElement>(COCKPIT_MAIN[section])
    expect(cockpit, `the ${section} cockpit did not render`).not.toBeNull()
    expect(document.querySelector('.view-crash')).toBeNull()
    await waitFor(() => expect(rail()!.querySelector('.rotor-az')?.textContent).toContain('100°T'))
    expect(railLine2(), 'an empty entry draws a line 2').toBeNull()
    const call = await waitFor(() => {
      const input = cockpit!.querySelector<HTMLInputElement>('.log-entry input.le-call')
      if (!input) throw new Error(`no log strip call box in ${section}`)
      return input
    })
    fireEvent.change(call, { target: { value: 'EC1DD' } })
    await waitFor(() => expect(railLine2Text()).toBe(line2For('EC1DD')), { timeout: 5000 })
    // Cleared, the line goes.
    fireEvent.change(call, { target: { value: '' } })
    await waitFor(() => expect(railLine2()).toBeNull(), { timeout: 5000 })
  })
})

describe('the cockpits with no log entry', () => {
  it.each(['sstv', 'aprs'].map((s) => [s]))('%s: the box, and no line 2', async (section) => {
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ [section]: true }))
    await mountOn(section)
    expect(document.querySelector(COCKPIT_MAIN[section]), `the ${section} cockpit did not render`).not.toBeNull()
    await waitFor(() => expect(rail()!.querySelector('.rotor-az')?.textContent).toContain('100°T'))
    expect(railLine2()).toBeNull()
    expect(api.rotatorBearingToCall).not.toHaveBeenCalled()
  })
})

describe('from afar', () => {
  it('the hosted Remote page, with the same records and the same QSO, draws no line 2 and asks for no bearing', async () => {
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ operate: true }))
    localStorage.setItem(
      'nexus.panels.operate.main',
      JSON.stringify({ v: 2, state: { box1: 'docked' }, share: {}, boxes: { box1: 'rotor' } }),
    )
    window.location.hash = '#operate'
    // Without station control, as the hosted page mounts App (remote-web/BrowserApplication).
    render(
      <StationControlContext.Provider value={false}>
        <App
          remote={{
            snapshot: { ...APP_SNAPSHOT, qso: { state: 'AwaitReport', dxcall: 'EC1DD', rxReport: null, running: false, cqRunning: false } } as unknown as AppSnapshot,
            settings: settingsFixture as unknown as Settings,
            bandPlan: [],
            status: <div>Observer</div>,
            cwPhone: true,
          }}
        />
      </StationControlContext.Provider>,
    )
    await screen.findByText('Observer')
    await act(async () => {})
    expect(document.querySelector('.app.remote-workspace'), 'premise: the Remote page mounted').not.toBeNull()
    expect(document.querySelector('.rotor-aim')).toBeNull()
    expect(api.rotatorBearingToCall).not.toHaveBeenCalled()
    cleanup()
  })
})
