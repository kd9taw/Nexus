// @vitest-environment jsdom
//
// THE TX STATE IS PAINTED THE SAME WAY EVERYWHERE (#482, #486). Paint only: nothing here reads or
// changes what keys the rig.
//
// #482: "TX off" on FT's strip was green and white on every other screen. FT's strip section wears
// `rx` whenever it is not on the air, TX off included, and `.cockpit-qso.rx .cq-statecap` painted
// its caption with the receive colour. The other screens' strip (CockpitTxStrip) carries the state
// on the caption itself. Measured as the cascade winner on the rendered elements, for both strips,
// in both themes: FT's caption must resolve to exactly what the shared strip's resolves to.
//
// #486: Settings ▸ Radio's "TX on" light stayed grey with transmit switched on, the same grey as
// "TX off". Solid green stays what it was, the path proven by power out during a tune; switched on
// but not yet proven is now the green ring, and TX off stays grey.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { createElement } from 'react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { CockpitTxStrip } from './components/CockpitTxStrip'
import { OperateQsoStrip } from './components/OperateQsoStrip'
import { SetupHealth } from './components/SetupHealth'
import { chainOf, parseRules, winnerAt, type El, type Mode } from './cssCascade'
import type { QsoStatus, RadioStatus } from './types'

vi.setConfig({ testTimeout: 15_000 })

vi.mock('./api', () => ({
  appVersion: vi.fn(() => Promise.resolve('0.0.0')),
  haltTx: vi.fn(() => Promise.resolve(null)),
  setFrequency: vi.fn(() => Promise.resolve(null)),
}))

afterEach(cleanup)

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))
const THEMES: readonly Mode[] = ["dark", "light"]

const RADIO = {
  dialMhz: 14.074, band: '20m', sideband: 'USB', catOk: true, txAllowed: true, transmitting: false,
  tuning: false, txBusyReason: null, rigKeyed: false, holdTxFreq: false, txEven: true, txCycleAuto: true,
}
const radio = (txEnabled: boolean) => ({ ...RADIO, txEnabled }) as unknown as RadioStatus

function capture(el: Element | null): El[] {
  expect(el, 'the element did not render').not.toBeNull()
  const chain = chainOf(el!)
  cleanup()
  return chain
}

function ftCaption(txEnabled: boolean): El[] {
  const noop = () => {}
  const qso = { running: false, state: 'Idle', dxcall: null, txNow: null } as unknown as QsoStatus
  const { container } = render(
    createElement(OperateQsoStrip, {
      qso, radio: radio(txEnabled), onSetMode: noop, onCallCq: noop, onResend: noop,
      onFreetext: () => true, onLog: noop, onSetTxEnabled: noop, onSetTune: noop, onHaltTx: noop,
      onSetHoldTxFreq: noop,
    } as unknown as Parameters<typeof OperateQsoStrip>[0]),
  )
  return capture(container.querySelector('.cq-statecap'))
}

function sharedCaption(txEnabled: boolean): El[] {
  const { container } = render(createElement(CockpitTxStrip, { radio: radio(txEnabled), onStopTx: () => {} }))
  return capture(container.querySelector('.cq-statecap'))
}

const colour = (chain: El[], theme: Mode) => winnerAt(RULES, theme, chain, 'color')?.value ?? null

describe('FT’s TX state caption is painted as every other screen’s (#482)', () => {
  it.each(THEMES)('TX off, %s theme: the same colour as the shared strip, not the receive green', (theme) => {
    const ft = colour(ftCaption(false), theme)
    const shared = colour(sharedCaption(false), theme)
    expect(ft).toBe(shared)
    expect(ft ?? '').not.toContain('--state-good')
  })

  it.each(THEMES)('CONTROL, TX armed, %s theme: both strips still say Receiving in green', (theme) => {
    const ft = colour(ftCaption(true), theme)
    expect(ft).toContain('--state-good')
    expect(ft).toBe(colour(sharedCaption(true), theme))
  })
})

function healthTxDot(over: { txEnabled: boolean; tuning?: boolean; txPower?: number | null }): El[] {
  const { container } = render(
    createElement(SetupHealth, { radio: { catOk: true, rxLevel: 0.01, audioError: null, ...over }, catResult: null }),
  )
  const items = container.querySelectorAll('.health-item')
  return capture(items[items.length - 1]?.querySelector('.health-dot') ?? null)
}

/** Every paint the dot can carry, as one string: its fill and its ring. */
const dotPaint = (chain: El[], theme: Mode) =>
  [
    winnerAt(RULES, theme, chain, 'background', 'background-color')?.value,
    winnerAt(RULES, theme, chain, 'box-shadow')?.value,
  ].join(' | ')

describe('Settings ▸ Radio’s TX light (#486)', () => {
  it.each(THEMES)('TX on, not yet proven, %s theme: green, and not the TX-off grey', (theme) => {
    const on = dotPaint(healthTxDot({ txEnabled: true }), theme)
    const off = dotPaint(healthTxDot({ txEnabled: false }), theme)
    expect(on).toContain('--state-good')
    expect(on).not.toBe(off)
  })

  it.each(THEMES)('and still distinct from the proven light (power seen during a tune), %s theme', (theme) => {
    const on = dotPaint(healthTxDot({ txEnabled: true }), theme)
    const proven = dotPaint(healthTxDot({ txEnabled: true, tuning: true, txPower: 5 }), theme)
    expect(proven).toContain('--state-good')
    expect(on).not.toBe(proven)
  })

  it.each(THEMES)('CONTROL, TX off, %s theme: grey, as before', (theme) => {
    const off = dotPaint(healthTxDot({ txEnabled: false }), theme)
    expect(off).toContain('--text-faint')
    expect(off).not.toContain('--state-good')
  })
})
