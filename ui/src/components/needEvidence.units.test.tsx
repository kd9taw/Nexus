// @vitest-environment jsdom
//
// THE "HEARD BY" LINE FOLLOWS THE UNITS SETTING.
//
// "heard by K9LC (EN52, 26 km)" was composed in Rust with the unit in it, so the Needed board's
// row tooltip and the Chase box's evidence line read in kilometres on Imperial. The backend now
// sends each receiver's distance as a km token (`km_token`, crates/propagation/src/geo.rs) and the
// screen writes the figure. In every case the setting and the OS locale DISAGREE, so a surface that
// read only one of them cannot pass: Imperial on a British locale, Metric on a US one.
//
// The evidence is the backend's own shape. 111.19… km is 69 mi and 166.9 km is 104 mi.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { cleanup, render } from '@testing-library/react'
import type { NeedAlert } from '../types'
import type { PaneContext } from './connect/paneContext'
import { setUnitsMirror, useUnits } from '../units'
import { NeededPanel } from './NeededPanel'
import { ChasePane } from './prop/ChasePane'

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  localStorage.clear()
})

const EVIDENCE = 'heard by K9LC (EN62, {km:111.19492664455873}) + N9CO (EN52, {km:166.9})'
const MILES = 'heard by K9LC (EN62, 69 mi) + N9CO (EN52, 104 mi)'
const KM = 'heard by K9LC (EN62, 111 km) + N9CO (EN52, 167 km)'

const NEED = {
  call: 'EA1ABC',
  entity: 'Spain',
  band: '20m',
  zone: 14,
  tags: ['NewBand'],
  priority: 40,
  headline: 'New band slot',
  mode: 'Digital',
  freqMhz: null,
  admittedAt: null,
  evidence: EVIDENCE,
} as unknown as NeedAlert

/** The Units setting as App stores it, and the OS locale the 'auto' setting would read. */
function units(setting: 'imperial' | 'metric', locale: string): void {
  vi.spyOn(navigator, 'language', 'get').mockReturnValue(locale)
  setUnitsMirror(setting)
}

function boardTooltip(): string {
  render(
    <NeededPanel alerts={[NEED]} bandPlan={[]} selectedCall={null} onQsy={() => {}} onSelect={() => {}} onWork={() => {}} />,
  )
  const row = document.querySelector('[title*="heard by"]')
  expect(row, 'the row tooltip carries the evidence line').toBeTruthy()
  return row!.getAttribute('title') ?? ''
}

/** The Chase box, given its units the way `usePaneContext` gives every Connect box: `useUnits()`. */
function ChaseBox() {
  const ctx = {
    myGrid: 'EN52',
    entityCentroids: new Map(),
    needAlerts: [NEED],
    bandOutlook: null,
    prop: null,
    dxpedWindows: new Map(),
    onSelectCall: () => {},
    units: useUnits(),
  } as unknown as PaneContext
  return <ChasePane ctx={ctx} />
}
function chaseLine(): string {
  render(<ChaseBox />)
  const line = document.querySelector('.chase-evi')
  expect(line, 'the Chase row shows its evidence line').toBeTruthy()
  return line!.textContent ?? ''
}

describe("the Needed board's evidence tooltip", () => {
  it('Imperial beats a British locale: miles', () => {
    units('imperial', 'en-GB')
    const tip = boardTooltip()
    expect(tip).toContain(MILES)
    expect(tip).not.toMatch(/km/)
  })

  it('Metric beats a US locale: kilometres', () => {
    units('metric', 'en-US')
    expect(boardTooltip()).toContain(KM)
  })
})

describe("the Chase box's evidence line", () => {
  it('Imperial beats a British locale: miles', () => {
    units('imperial', 'en-GB')
    expect(chaseLine()).toBe(MILES)
  })

  it('Metric beats a US locale: kilometres', () => {
    units('metric', 'en-US')
    expect(chaseLine()).toBe(KM)
  })

  it('passes an evidence line with no distance through untouched', () => {
    units('imperial', 'en-GB')
    NEED.evidence = 'spotted by K9IMM via RBN'
    try {
      expect(chaseLine()).toBe('spotted by K9IMM via RBN')
    } finally {
      NEED.evidence = EVIDENCE
    }
  })
})
