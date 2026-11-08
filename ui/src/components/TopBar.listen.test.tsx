// @vitest-environment jsdom
//
// #468: while receive audio plays on this computer, the top bar's RX level meter is the listening
// level's button; otherwise it is the meter alone. Either way the bar's line gets nothing new: the
// trigger's box properties all resolve to nothing (cascade winners over both sheets, on the rendered
// chain), so the button is exactly the meter's box. A slider, and then a 25 px chip, beside Help wrapped
// the bar at 1366x768 on FT and moved Stop TX down.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { createElement } from 'react'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'
import { TopBar } from './TopBar'
import { chainOf, parseRules, winnerAt } from '../cssCascade'
import type { RadioStatus } from '../types'

vi.mock('../api', () => ({
  appVersion: vi.fn(() => Promise.resolve('0.0.0')),
  haltTx: vi.fn(() => Promise.resolve(null)),
  setFrequency: vi.fn(() => Promise.resolve(null)),
}))
afterEach(cleanup)

const RADIO = { dialMhz: 14.074, band: '20m', sideband: 'USB', txEnabled: true, txAllowed: true, tuning: false,
  transmitting: false, txEven: true, txCycleAuto: true, holdTxFreq: false, catOk: true, dtSec: 0, clockOffsetMs: 0 }

function bar(extra: Record<string, unknown>) {
  const noop = () => {}
  return render(
    createElement(TopBar, {
      mycall: 'KD9TAW', mygrid: 'EN61', radio: RADIO as unknown as RadioStatus, link: { tier: 'FT8' }, bandPlan: [],
      onSetFrequency: noop, onSetTxEnabled: noop, onSetTune: noop, onHaltTx: noop, onSetTxEven: noop,
      onSetTxCycleAuto: noop, onSetHoldTxFreq: noop, tier: 'FT8', onTierChange: noop, onOpenGuide: noop,
      ...extra,
    } as unknown as Parameters<typeof TopBar>[0]),
  ).container
}

const sheet = (name: string) =>
  readFileSync(resolve(process.cwd(), 'src', name), 'utf8').replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '))
const RULES = parseRules(sheet('styles.css') + '\n' + sheet('cockpit-panes.css'))

describe('the top bar\u2019s listening level (#468)', () => {
  it.each([
    ['an FT screen', {}],
    ['a cockpit (Phone, CW, RTTY…)', { hideTxControls: true, hideFrequencyControl: true, hideDigitalChrome: true }],
  ])('on %s, the RX meter is the button, and nothing is added to the chip group', (_where, extra) => {
    const plain = bar(extra).querySelector('.topbar-chips')!.children.length
    cleanup()
    const c = bar({ ...extra, listenLevel: { level: 0.3, onCommit: async () => {} } })
    const btn = c.querySelector('.txrx button.rx-level.listen-trigger')
    expect(btn).not.toBeNull()
    expect(btn!.querySelector('.rx-level-label')).not.toBeNull()
    expect(c.querySelector('.topbar-chips')!.children.length).toBe(plain)
    expect(c.querySelector('.topbar input')).toBeNull()
  })

  it.each(['dark', 'light'] as const)('the button brings no box of its own, %s theme', (theme) => {
    const c = bar({ listenLevel: { level: 0.3, onCommit: async () => {} } })
    const chain = chainOf(c.querySelector('button.listen-trigger')!)
    const zero = (v: string | undefined) => v != null && /^(0|0px|none|0 none)$/.test(v.trim())
    for (const props of [['padding', 'padding-left', 'padding-right', 'padding-top', 'padding-bottom'], ['border', 'border-width'], ['margin']])
      expect(zero(winnerAt(RULES, theme, chain, ...props)?.value), props[0]).toBe(true)
  })

  it('CONTROL: without it the meter is a meter, not a button', () => {
    const c = bar({})
    expect(c.querySelector('.listen-trigger')).toBeNull()
    expect(c.querySelector('.txrx div.rx-level')).not.toBeNull()
  })
})
