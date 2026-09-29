// @vitest-environment jsdom
//
// AWARDS' COLUMN DIVIDER (layout L7): the boundary between the progress column (DXCC and grids by
// band, DXCC by mode) and the chase lists. What the view wires: the divider in the grid, the stored
// split per window, a reset, and a window that never set one. jsdom lays nothing out, so the two
// columns get stubbed widths; the divider's own arithmetic is PaneSeam's (PaneSeam.test.tsx), the
// template the cascade's (view-grids.test.ts), and the rectangles Chrome's (the layout harness).
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import { AwardsView } from './AwardsView'
import type { AwardSummary } from '../types'

const SUMMARY: Partial<AwardSummary> = {
  qsos: 31648, confirmedQsos: 15881, dxccWorked: 332, dxccConfirmed: 330, dxccCredited: 197, readyToSubmit: 133,
  slotsWorked: 2702, slotsConfirmed: 2552,
  bands: [{ band: '20m', worked: 300, confirmed: 280 }],
  modes: [], needed: [], slotNeeded: [], achievements: [],
  fiveBandWorked: 244, fiveBandConfirmed: 225, satDxccWorked: 16, satDxccConfirmed: 16, wazWorked: 40, wazConfirmed: 40,
  honorRoll: { currentTotal: 340, confirmed: 330, threshold: 331, achieved: false, needed: 1, numberOne: false, numberOneNeeded: 10 },
  was: { worked: 50, confirmed: 50, needed: [], fiveBandWorked: 50, fiveBandConfirmed: 50 },
  vucc: { worked: 1602, confirmed: 1489, bands: [], satWorked: 63, satConfirmed: 3, awards: [] },
  iota: { worked: 302, confirmed: 286, cardConfirmed: 41 },
  bandTargets: [],
}

vi.mock('../api', () => ({
  getAwards: vi.fn(() => Promise.resolve(SUMMARY)),
  getConfirmationDiagnostics: vi.fn(() => Promise.resolve(null)),
  uploadLotw: vi.fn(),
  uploadQrz: vi.fn(),
  uploadClublog: vi.fn(),
  uploadEqsl: vi.fn(),
  pushQsoQrz: vi.fn(),
  pushQsoClublog: vi.fn(),
  pushQsoEqsl: vi.fn(),
}))

const KEY = 'nexus.awards.columns'
const LEFT_W = 600
const CHASES_W = 460

const realRect = HTMLElement.prototype.getBoundingClientRect
beforeEach(() => {
  localStorage.clear()
  HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
    const box = (left: number, width: number) => ({ x: left, y: 0, left, top: 0, width, height: 600, right: left + width, bottom: 600, toJSON() {} }) as DOMRect
    if (this.classList.contains('aw-left')) return box(0, LEFT_W)
    if (this.classList.contains('aw-chases')) return box(LEFT_W + 16, CHASES_W)
    return realRect.call(this)
  }
})
afterEach(() => {
  cleanup()
  HTMLElement.prototype.getBoundingClientRect = realRect
})

async function mount() {
  const r = render(<AwardsView />)
  await waitFor(() => expect(document.querySelector('.awards-body')).not.toBeNull())
  return r.container
}

const body = (c: HTMLElement) => c.querySelector<HTMLElement>('.awards-body')!
const divider = () => screen.getByRole('separator', { name: 'Progress column / chase lists' })
const token = (c: HTMLElement, v: 'a' | 'b') => body(c).style.getPropertyValue(`--awards-col-${v}`)

describe('Awards’ column divider', () => {
  it('is the grid’s own child after the two columns, announcing the split on screen', async () => {
    const c = await mount()
    const d = divider()
    expect(d.parentElement).toBe(body(c))
    expect([...body(c).children].map((e) => e.className)).toEqual(['aw-left', 'aw-chases', expect.stringContaining('awards-colseam')])
    await waitFor(() => expect(d.getAttribute('aria-valuenow')).toBe('57'))
  })

  it('a window that never set a split opens exactly as before; a key stores one; Backspace takes it back', async () => {
    const c = await mount()
    expect(token(c, 'a')).toBe('')
    fireEvent.keyDown(divider(), { key: 'ArrowLeft' })
    const a = 2 * (LEFT_W / (LEFT_W + CHASES_W) - 0.05)
    expect(parseFloat(token(c, 'a'))).toBeCloseTo(a, 9)
    expect(parseFloat(token(c, 'b'))).toBeCloseTo(2 - a, 9)
    expect(Number(localStorage.getItem(KEY))).toBeCloseTo(a, 9)
    fireEvent.keyDown(divider(), { key: 'Backspace' })
    expect(token(c, 'a')).toBe('')
    expect(localStorage.getItem(KEY)).toBe('')
  })

  it('a stored split opens where the operator left it', async () => {
    localStorage.setItem(KEY, '0.9')
    const c = await mount()
    expect(token(c, 'a')).toBe('0.9fr')
    expect(token(c, 'b')).toBe('1.1fr')
  })
})
