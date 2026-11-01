// @vitest-environment jsdom
//
// THE AMPLIFIER BOX IN THE DASHBOARD POP-OUT LOCKS WHILE THE TRANSMITTER IS ON THE AIR. The pop-out
// (DetachedPanel `connect`) is the one host of the box outside the main window: it reads the shared
// snapshot itself and hands Connect the radio's ON AIR answer beside the amplifier. App.ampLock.test.tsx
// holds the main window's three paths; this is the fourth. Same shape: an Elecraft (no transmit flag of
// its own), on the air by the arbiter only, so only the host's answer can lock the button; and its
// control, off the air, live.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { act, cleanup, render, waitFor } from '@testing-library/react'

const fx = vi.hoisted(() => ({ snap: null as unknown }))

vi.mock('./api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  const { appApiAnswers } = await import('./appCockpits.testkit')
  return {
    ...auto,
    ...appApiAnswers(),
    getSnapshot: vi.fn(async () => fx.snap),
    subscribeSnapshot: vi.fn((cb: (s: unknown) => void) => {
      cb(fx.snap)
      return () => {}
    }),
    getSettings: vi.fn(async () => null),
  }
})
vi.mock('./components/MapView', () => ({ MapView: () => <div data-testid="map" /> }))

import { DetachedPanel } from './DetachedPanel'
import { APP_SNAPSHOT } from './appCockpits.testkit'
import { pastTheSwitch } from './components/ConnectView.testkit'

vi.setConfig({ testTimeout: 30_000 })

const AMP = {
  family: 'kpa', model: '', linked: true, reason: '', operate: true, transmitting: null, outputWatts: 0,
  swr: 1.1, swrAtu: null, volts: 48, amps: 0, temp: 30, tempCelsius: true, alarm: 'none', alarmRaised: false,
  warning: 'none', warningRaised: false, kpaFault: null, bandLabel: null,
}
const snapshot = (onAir: boolean) => ({
  ...APP_SNAPSHOT,
  radio: {
    ...APP_SNAPSHOT.radio,
    amp: AMP,
    txBusyReason: onAir ? 'A voice message is transmitting — stop it first' : null,
    rigKeyed: false,
  },
})

beforeEach(() => {
  localStorage.clear()
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
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  window.matchMedia = ((q: string) =>
    ({ matches: false, media: q, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} }) as unknown as MediaQueryList) as typeof window.matchMedia
})
afterEach(cleanup)

describe('the dashboard pop-out’s Amplifier box', () => {
  it.each([
    ['on the air: locked', true],
    ['off the air (the control): live', false],
  ] as const)('%s', async (_what, onAir) => {
    fx.snap = snapshot(onAir)
    await act(async () => {
      render(<DetachedPanel panel="connect" />)
    })
    const b = await waitFor(
      () => {
        const el = document.querySelector<HTMLButtonElement>('.amp-opbtn')
        if (!el) throw new Error('no Amplifier box button in the pop-out')
        return el
      },
      { timeout: 5000 },
    )
    expect(b.disabled).toBe(onAir)
  })
})
