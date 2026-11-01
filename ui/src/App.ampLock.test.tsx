// @vitest-environment jsdom
//
// THE AMPLIFIER BOX'S OPERATE/STANDBY LOCKS WHENEVER THE TRANSMITTER IS ON THE AIR — IN EVERY PLACE THE
// REAL APP DRAWS THE BOX. The box's counterpart of CockpitHeader.ampgate.test.tsx.
//
// The button (prop/AmpPane) obeys the `radioOnAir` it is handed (AmpPane.operate.test.tsx); what its own
// tests cannot see is whether each host HANDS IT the right answer. Three paths reach it in the main
// window, each a different wiring from App's snapshot: the dashboard rail beside a cockpit (App's
// `boxSource` → DashRail), a box standing in a cockpit's own columns (`boxSource` → CockpitBox → BoxBody),
// and Connect (App → ConnectView). All three go through the pane registry (connect/panes). The pop-out's
// path is DetachedPanel.ampLock.test.tsx.
//
// ON THE AIR HERE IS THE ARBITER ONLY: `transmitting` false, the rig's read-back false, `txBusyReason`
// set — a voice over, as the snapshot reports one. The amplifier is an Elecraft, which reports no transmit
// flag of its own, so the radio's ON AIR answer is the ONLY thing that can lock the button: a host that
// lends `false`, or the FT slot flag alone, leaves it live, and these go red. Each case has its control:
// the same box, the radio off the air, the button live.
import { describe, it, expect, vi, beforeEach } from 'vitest'
import { act, render, waitFor } from '@testing-library/react'
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
import { APP_SNAPSHOT, COCKPIT_MAIN } from './appCockpits.testkit'
import { pastTheSwitch } from './components/ConnectView.testkit'

// Each case mounts the real App; under the full suite's load that outruns vitest's default 5 s.
vi.setConfig({ testTimeout: 30_000 })

const SECTIONS_ON = { phone: true, cw: true, rtty: true, psk: true, sstv: true, aprs: true, js8: true, connect: true }

/** A linked Elecraft in Operate: no transmit flag of its own, so only the radio can lock the button. */
const AMP = {
  family: 'kpa', model: '', linked: true, reason: '', operate: true, transmitting: null, outputWatts: 0,
  swr: 1.1, swrAtu: null, volts: 48, amps: 0, temp: 30, tempCelsius: true, alarm: 'none', alarmRaised: false,
  warning: 'none', warningRaised: false, kpaFault: null, bandLabel: null,
}
/** The snapshot, the radio on the air (a voice over: only the arbiter says so) or not. */
const snapshot = (onAir: boolean) => ({
  ...APP_SNAPSHOT,
  radio: {
    ...APP_SNAPSHOT.radio,
    amp: AMP,
    txBusyReason: onAir ? 'A voice message is transmitting — stop it first' : null,
    rigKeyed: false,
  },
})

function serve(snap: unknown): void {
  for (const answer of [api.getSnapshot, api.setOperatingMode, api.setArea, api.selectPeer]) {
    vi.mocked(answer as Mock).mockImplementation(async () => snap)
  }
}

async function mountOn(view: string): Promise<void> {
  localStorage.setItem('nexus.workspace', 'dx')
  window.location.hash = `#${view}`
  render(<App />)
  await waitFor(() => expect(document.querySelector('.app.loading')).toBeNull())
  await waitFor(() => expect(document.documentElement.getAttribute('data-viewport')).not.toBeNull())
  await act(async () => {})
}

/** The box's Operate/Standby button under `scope`, once the snapshot has reached it. */
async function boxButton(scope: string): Promise<HTMLButtonElement> {
  return waitFor(
    () => {
      const b = document.querySelector<HTMLButtonElement>(`${scope} .amp-opbtn`)
      if (!b) throw new Error(`no Amplifier box button under ${scope}`)
      return b
    },
    { timeout: 5000 },
  )
}

beforeEach(() => {
  localStorage.clear()
  localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: SECTIONS_ON }))
  document.documentElement.removeAttribute('data-viewport')
  Object.defineProperty(window, 'innerWidth', { value: 1920, configurable: true })
  Object.defineProperty(window, 'innerHeight', { value: 1080, configurable: true })
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})

const BOTH: Array<[string, boolean]> = [
  ['on the air: locked', true],
  ['off the air (the control): live', false],
]

describe('the dashboard rail beside a cockpit', () => {
  it.each(BOTH)('%s', async (_what, onAir) => {
    serve(snapshot(onAir))
    localStorage.setItem('nexus.dashrail.config', JSON.stringify({ slots: { rail1: 'amp', rail2: 'clock', rail3: 'spacewx', rail4: 'getout' } }))
    localStorage.setItem('nexus.dashrail.sections', JSON.stringify({ phone: true }))
    await mountOn('phone')
    expect(document.querySelector(COCKPIT_MAIN.phone), 'the Phone cockpit did not render').not.toBeNull()
    const b = await boxButton('.dash-rail')
    expect(b.disabled).toBe(onAir)
  })
})

describe('a box in a cockpit’s own columns', () => {
  it.each(BOTH)('%s', async (_what, onAir) => {
    serve(snapshot(onAir))
    localStorage.setItem(
      'nexus.panels.operate.main',
      JSON.stringify({ v: 2, state: { box1: 'docked', recall: 'removed' }, share: {}, boxes: { box1: 'amp' } }),
    )
    await mountOn('operate')
    const b = await boxButton(`${COCKPIT_MAIN.operate} .pane-frame[data-pane="box1"]`)
    expect(b.disabled).toBe(onAir)
  })
})

describe('Connect', () => {
  it.each(BOTH)('%s', async (_what, onAir) => {
    serve(snapshot(onAir))
    // Past the one-time switch to the default layout, so the placement below is the one on screen.
    pastTheSwitch()
    localStorage.setItem('nexus.connect.autopop', 'off')
    localStorage.setItem('nexus.needed.autopop', 'off')
    localStorage.setItem('nexus.connect.chaseDefault.v1', '1')
    localStorage.setItem(
      'nexus.connect.config',
      JSON.stringify({
        slots: { left1: 'amp', left2: 'bandTiles', right1: 'chase', right2: 'outlook', bottom1: 'openings', bottom2: 'spacewx', bottom3: 'getout' },
        overlays: {},
      }),
    )
    await mountOn('connect')
    const b = await boxButton('.pane-frame[data-pane="amp"]')
    expect(b.disabled).toBe(onAir)
  })
})
