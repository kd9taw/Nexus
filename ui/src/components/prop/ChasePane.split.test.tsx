// @vitest-environment jsdom
//
// A CHASE ROW'S ENTITY TAKES A LINE OF ITS OWN WHEN ITS FIRST LINE HAS NO ROOM FOR IT (the operator's
// ruling F2). A Chase row's first line is the need chip, the call, the ↗ point button, the entity with
// its beam heading, and the age. In a Chase box about 300 px wide Chrome measured the entity
// ("South Orkney Is.") cut to 47 px at 100 % and to 0 px at 160 %. Where the entity and its heading do not
// fit beside the rest, the head is stamped `data-split` and they go under it, while the call, the chip,
// the ↗ and the age stay on the first line. The Chase Feed pane shares the row, and the fix.
//
// jsdom lays nothing out, so the widths the stamp reads are given here, and the layout the stamp selects
// is the cascade winner over the real sheets (cssCascade.testkit). The geometry is the real-browser
// census's.
import { describe, it, expect, beforeAll, beforeEach, afterEach, vi } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import type { NeedAlert } from '../../types'
import type { PaneContext } from '../connect/paneContext'
import { css, loadSheets } from '../../cssCascade.testkit'
import { ChasePane } from './ChasePane'
import { ChaseFeedPane } from './ChaseFeedPane'

// THE BUDGET (2026-10-09). The slowest case here, "stamps its head, and the entity and its heading take…", takes
// 0.25 s and 0.67 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const NEED = {
  call: 'VP8ORK', entity: 'South Orkney Is.', band: '20m', zone: 13, tags: ['NewEntity'], priority: 90,
  headline: 'VP8ORK needed', mode: 'CW', freqMhz: 14.025, admittedAt: Math.floor(Date.now() / 1000) - 120,
} as unknown as NeedAlert

const ctx = {
  myGrid: 'EN52',
  entityCentroids: new Map([['South Orkney Is.', { lat: -60.6, lon: -45.5 }]]),
  needAlerts: [NEED],
  bandOutlook: null,
  prop: null,
  dxpedWindows: new Map(),
  onSelectCall: () => {},
  onPoint: () => {},
} as unknown as PaneContext

/** The widths a laid-out row would have: the head's, each item's, and the entity's full text. */
const geometry = { head: 0 }
const WIDTH: Record<string, number> = { 'need-chip': 58, 'chase-call': 60, 'np-point': 20, 'chase-age': 24, 'chase-az': 30, 'cfeed-rank': 12 }

beforeAll(() => loadSheets())
beforeEach(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Object.defineProperty(HTMLElement.prototype, 'clientWidth', {
    configurable: true,
    get(this: HTMLElement) {
      return this.classList.contains('chase-head') ? geometry.head : 0
    },
  })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', {
    configurable: true,
    get(this: HTMLElement) {
      return Object.entries(WIDTH).find(([cls]) => this.classList.contains(cls))?.[1] ?? 0
    },
  })
  Object.defineProperty(HTMLElement.prototype, 'scrollWidth', {
    configurable: true,
    get(this: HTMLElement) {
      return this.classList.contains('chase-entity') ? 110 : 0
    },
  })
})
afterEach(() => {
  cleanup()
  for (const p of ['clientWidth', 'offsetWidth', 'scrollWidth']) delete (HTMLElement.prototype as unknown as Record<string, unknown>)[p]
})

describe('a Chase row whose first line has no room for the entity', () => {
  it('stamps its head, and the entity and its heading take the line under it', () => {
    geometry.head = 200 // 162 px of chip, call, ↗ and age: 38 px left for a 140 px entity and heading
    render(<ChasePane ctx={ctx} />)
    const head = document.querySelector<HTMLElement>('.chase-head')!
    expect(head.querySelector('.chase-entity')?.textContent, 'control: the row draws the entity').toBe('South Orkney Is.')
    expect(head.hasAttribute('data-split'), 'the entity stays cut beside the call').toBe(true)
    const place = head.querySelector<HTMLElement>('.chase-where')!
    expect(place.contains(head.querySelector('.chase-az')), 'the heading does not go with the entity').toBe(true)
    expect(css(head, 'flex-wrap'), 'the head keeps one line').toBe('wrap')
    expect(css(place, 'order'), 'the entity is not after the age').toBe('1')
    expect(css(place, 'flex-basis'), 'the entity does not take a line of its own').toBe('100%')
    // On its own line the country may still be wider than a 200 px box leaves it ("South Orkney Is." and
    // its heading, 129 px in 115, measured in Chrome): there it wraps between its words, whole.
    expect(css(head.querySelector('.chase-entity')!, 'white-space'), 'the country is cut on its own line').toBe('normal')
  })

  it('keeps one line where the entity fits beside the rest', () => {
    geometry.head = 600
    render(<ChasePane ctx={ctx} />)
    const head = document.querySelector<HTMLElement>('.chase-head')!
    expect(head.hasAttribute('data-split'), 'a row with room split anyway').toBe(false)
    expect(css(head, 'flex-wrap')).toBeNull()
    expect(css(head.querySelector('.chase-where')!, 'order')).toBeNull()
    expect(css(head.querySelector('.chase-entity')!, 'white-space'), 'beside the call the country keeps one line').toBe('nowrap')
  })

  it('does the same in the Chase Feed pane, which shares the row', () => {
    geometry.head = 150
    render(<ChaseFeedPane ctx={ctx} />)
    const head = document.querySelector<HTMLElement>('.chase-head')
    expect(head, 'control: the feed lists the need').not.toBeNull()
    expect(head!.hasAttribute('data-split')).toBe(true)
  })
})
