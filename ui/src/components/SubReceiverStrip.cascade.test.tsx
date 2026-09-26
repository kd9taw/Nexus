// @vitest-environment jsdom
//
// THE SUB STRIP SIZES NOTHING — computed over the REAL sheets, never read off them.
//
// It lives inside Phone's receiver pane, which is `fit="content"`: exactly content height, so
// the wrap of its rows IS the height. The layout contract (CLAUDE.md, the header of
// `cockpit-panes.css`) says a pane never sizes itself and structural size lives in
// `cockpit-panes.css` alone — so the strip may declare no height, no floor, no basis and no
// clip of its own, and it must lay its rows out exactly as Main's chain does (it IS a
// `.ph-chain`). The one thing it adds is the rule that says a different receiver starts here.
//
// Every assertion resolves the cascade WINNER for the rendered element (cssCascade.testkit:
// importance → specificity → source order, the real selector engine) — a regex over the sheet
// would pass on a dead selector, which is how two dead fixes shipped before the overhaul.
// ⚠️ jsdom does not lay out: nothing here is a pixel, only what the cascade declares.
import { describe, it, expect, beforeAll, afterEach, vi } from 'vitest'
import { render, cleanup } from '@testing-library/react'
import { loadSheets, css, pxOf } from '../cssCascade.testkit'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'

// The REAL host, so a rule scoped by where the strip sits (`.pane-body …`, the pane frame, the
// cockpit) competes exactly as it would on screen — the strip rendered alone would be blind to
// every one of them.
vi.mock('../api', async original => {
  const actual = await original<Record<string, unknown>>()
  const reads: Record<string, unknown> = { getLicensedBandPlan: [], getBandPlan: [], getCatCwUnprovenRigModels: [] }
  return Object.fromEntries(Object.entries(actual).map(([name, value]) => [name,
    typeof value === 'function' ? vi.fn(async () => structuredClone(reads[name] ?? {})) : value]))
})
vi.mock('../toast', () => ({ pushToast: vi.fn(), withErrorToast: vi.fn(async (a: () => Promise<unknown>) => a()) }))
vi.mock('./PhoneScope', () => ({ PhoneScope: () => <div /> }))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeAll(() => {
  globalThis.ResizeObserver = class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver
  Element.prototype.scrollIntoView = vi.fn()
  loadSheets()
})
afterEach(cleanup)

const snap = {
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  radio: {
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
    rfGain: 1,
    afGain: 0.5,
    squelch: 0,
    receivers: {
      main: { id: 'main', stages: { frontEnd: 'own', dsp: 'own', audio: 'own' } },
      sub: { id: 'sub', stages: { frontEnd: 'own', dsp: 'unknown', audio: 'own' }, dialMhz: 145.965, band: '2m', sideband: 'LSB' },
      subCapability: 'present',
      subCommandable: true,
    },
  },
  link: {},
  qso: {},
  stations: [],
  conversations: [],
} as unknown as AppSnapshot

function strip() {
  render(<PhoneCockpit snap={snap} theme="dark" />)
  const el = document.querySelector('[data-pane="receiver"] .ph-subrx')
  expect(el, 'the strip rendered in the receiver pane').not.toBeNull()
  return el!
}

describe('the Sub strip in a fit="content" pane', () => {
  it('lays its rows out as Main’s chain does — it is a .ph-chain', () => {
    const el = strip()
    expect(css(el, 'display')).toBe('flex')
    expect(css(el, 'flex-wrap')).toBe('wrap')
    // Parser sanity, and the discriminator for the "declares nothing" checks below: the same
    // resolver DOES find the strip's own declaration, so a null there is an absence, not blindness.
    expect(pxOf(el, 'border-top-width'), 'the rule that says a different receiver starts here').toBe(1)
  })

  it('declares no size, floor, basis or clip of its own', () => {
    const el = strip()
    for (const prop of ['height', 'min-height', 'max-height', 'flex-basis', 'flex', 'overflow', 'overflow-y']) {
      expect(css(el, prop), `.ph-subrx must not set ${prop}`).toBeNull()
    }
    for (const row of el.querySelectorAll('.ph-chain-item')) {
      for (const prop of ['height', 'min-height', 'flex-basis']) {
        expect(css(row, prop), `a Sub row must not set ${prop}`).toBeNull()
      }
    }
  })
})
