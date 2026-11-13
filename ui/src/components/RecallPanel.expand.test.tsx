// @vitest-environment jsdom
// THE ⤢ — the capped recall card opens in full (tester report, 2026-11-13). What jsdom can hold: it opens,
// it closes on Esc, on its own ✕, on a press outside and on the ⤢ again; a press inside does not close
// it; the avatar's QRZ button still works in it; it is portaled to <body>, never inside the TX dock; Esc is
// never prevented (it is the stop key too); and the keyboard lands in it without scrolling anything.
// Where it stands on screen is geometry: `scripts/browser-probe --desktop recallcard`.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { act, render, screen, cleanup, fireEvent } from '@testing-library/react'
import { RecallPanel } from './RecallPanel'
import { openQrzPage } from '../api'
import type { CallHistory } from '../features/callHistory'
import type { CallBearing } from '../types'

const api = vi.hoisted(() => ({
  openQrzPage: vi.fn(async () => {}),
  rotatorBearingToCall: vi.fn((_call: string): Promise<CallBearing> => Promise.reject(new Error('unknownStation'))),
}))
vi.mock('../api', () => api)

const HIST: CallHistory = {
  qsos: [{ call: 'N9BNE', band: '20m', mode: 'FT8', rstSent: '-07', rstRcvd: '-12', whenUnix: Date.UTC(2026, 2, 14) / 1000 } as never],
  count: 1,
  workedBefore: true,
  dupeThisBand: false,
  lastUnix: Date.UTC(2026, 2, 14) / 1000,
  confirmedCount: 1,
  bands: ['20m'],
  modes: ['FT8'],
}

/** A cockpit as far as the card cares: the pane region with the card, and the TX dock below it. */
function cockpit(bounded = true) {
  return render(
    <main className="operate-cockpit">
      <div className="cockpit-side">
        <RecallPanel call="N9BNE" name="Barry N Evans" qth="Indianapolis" state="IN" grid="EM69vu" hist={HIST} hasLookup={false} bounded={bounded} />
      </div>
      <div className="cockpit-txdock" data-testid="dock">
        <button type="button">PTT</button>
      </div>
    </main>,
  )
}

const expand = () => screen.getByRole('button', { name: "Open N9BNE's whole card" })
const full = () => screen.queryByRole('dialog', { name: 'N9BNE: the whole card' })

// jsdom lays nothing out: give the boxes a layout (the dock at the foot of a 1024×768 window, everything
// else a modest box at the top) so the card's placement has room to place it.
const realRect = Element.prototype.getBoundingClientRect
beforeEach(() => {
  Element.prototype.getBoundingClientRect = function (this: Element) {
    const dock = this.classList.contains('cockpit-txdock')
    const r = dock ? { left: 0, top: 700, width: 1024, height: 60 } : { left: 40, top: 40, width: 300, height: 120 }
    return { ...r, x: r.left, y: r.top, right: r.left + r.width, bottom: r.top + r.height, toJSON: () => r } as DOMRect
  }
})
afterEach(() => {
  Element.prototype.getBoundingClientRect = realRect
  cleanup()
  vi.restoreAllMocks()
  api.openQrzPage.mockClear()
})

describe('RecallPanel — the ⤢ opens the whole card', () => {
  it('opens a dialog with the whole card, portaled to <body> and never inside the TX dock', () => {
    cockpit()
    expect(full()).toBeNull()
    fireEvent.click(expand())
    const d = full()
    expect(d).not.toBeNull()
    expect(expand().getAttribute('aria-expanded')).toBe('true')
    expect(d!.parentElement).toBe(document.body)
    expect(d!.closest('.cockpit-txdock')).toBeNull()
    expect(screen.getByTestId('dock').contains(d)).toBe(false)
    expect(d!.closest('main')).toBeNull()
    // The whole card: the name, the place and the prior contact, with the photo's avatar.
    expect(d!.textContent).toContain('Barry N Evans')
    expect(d!.textContent).toContain('Indianapolis, IN')
    expect(d!.querySelectorAll('[role="listitem"]')).toHaveLength(1)
    expect(d!.querySelector('.recall-card-full .recall-avatar')).not.toBeNull()
    // It has no ⤢ of its own and no pane ✕: its box closes.
    expect(d!.querySelector('.recall-expand')).toBeNull()
    expect(d!.querySelector('.cockpit-popout')).toBeNull()
  })

  it('is placed off the TX dock (the station card rule), and shows', () => {
    cockpit()
    fireEvent.click(expand())
    const d = full()!
    expect(d.dataset.placed).toBe('yes')
    const top = parseFloat(d.style.top)
    const maxH = parseFloat(d.style.maxHeight)
    expect(top + maxH).toBeLessThanOrEqual(700)
  })

  it('closes on Esc, and never prevents the key (Esc is the stop key too)', () => {
    cockpit()
    fireEvent.click(expand())
    const ev = new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true })
    act(() => {
      window.dispatchEvent(ev)
    })
    expect(ev.defaultPrevented).toBe(false)
    expect(full()).toBeNull()
  })

  it('closes on a press outside it, the TX dock included, and not on a press inside it', () => {
    cockpit()
    fireEvent.click(expand())
    fireEvent.pointerDown(full()!.querySelector('.recall-name')!)
    expect(full()).not.toBeNull()
    fireEvent.pointerDown(screen.getByRole('button', { name: 'PTT' }))
    expect(full()).toBeNull()
    fireEvent.click(expand())
    fireEvent.pointerDown(document.body)
    expect(full()).toBeNull()
  })

  it('closes on its own ✕ and on the ⤢ again', () => {
    cockpit()
    fireEvent.click(expand())
    fireEvent.click(screen.getByRole('button', { name: 'Close the whole card' }))
    expect(full()).toBeNull()
    fireEvent.click(expand())
    expect(full()).not.toBeNull()
    fireEvent.pointerDown(expand())
    fireEvent.click(expand())
    expect(full()).toBeNull()
  })

  it("keeps the avatar's QRZ button working inside the whole card", () => {
    cockpit()
    fireEvent.click(expand())
    const qrz = full()!.querySelector<HTMLButtonElement>('button.recall-avatar')!
    fireEvent.pointerDown(qrz)
    fireEvent.click(qrz)
    expect(openQrzPage).toHaveBeenCalledWith('N9BNE')
    expect(full()).not.toBeNull()
  })

  it('opened from the keyboard, takes the keyboard without scrolling anything', () => {
    const focus = vi.spyOn(HTMLElement.prototype, 'focus')
    cockpit()
    // A keyboard activation is a click with no pointer detail.
    fireEvent.click(expand(), { detail: 0 })
    const d = full()!
    expect(focus.mock.contexts).toContain(d)
    const call = focus.mock.calls[focus.mock.contexts.indexOf(d)]
    expect(call[0]).toEqual({ preventScroll: true })
  })

  it('is absent where the card is not capped (the CW / Phone log panes draw it whole already)', () => {
    cockpit(false)
    expect(screen.queryByRole('button', { name: "Open N9BNE's whole card" })).toBeNull()
  })

  it('gives the name its whole text as a tooltip, for when the rail is too narrow for it', () => {
    cockpit()
    expect(document.querySelector('.recall-name')!.getAttribute('title')).toBe('Barry N Evans')
  })
})
