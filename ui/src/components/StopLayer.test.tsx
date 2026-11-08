// @vitest-environment jsdom
//
// STOP TX ABOVE EVERY OVERLAY, in jsdom (the ruling "always clickable on top"; the layer is components/StopLayer.tsx).
//
// For each kind of overlay the app draws over Stop TX — a ui/Dialog (Radix, which locks the page's pointer events and
// traps the keyboard), a ConfirmHost question (the same, through confirmDialog), the prompt to log (a hand-rolled
// full-window scrim), the launch radio picker (another, at z 3000), an aria-modal `.logconfirm-backdrop` of the
// Logbook's kind, and an open menu drawn over the button — ONE click where Stop TX is drawn must stop exactly once and
// leave the overlay open, and the keyboard must stay in a dialog that had it.
//
// HOW "WHERE STOP TX IS DRAWN" IS FOUND. jsdom lays nothing out, so the page is given a geometry (`layout` below):
// Stop TX has a box, every overlay covers the window, and `elementsFromPoint` answers the overlay that was drawn last
// over a point, the way the stacking reads in this app (every overlay portals or is drawn after the screen). The click
// then goes where a browser sends it: to the stop layer's mirror of Stop TX when there is one (the layer's z-index
// outranks every other one, which stop-layer.test.ts computes from the stylesheets), and otherwise to whatever is
// topmost at Stop TX's centre, under `pointer-events: none` no element at all (<html>), as Chrome hit-tests.
// The real-Chrome half is `scripts/browser-probe --desktop stopabove`.
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { useState, type ReactNode } from 'react'
import { StopLayer } from './StopLayer'
import { Dialog } from './ui/Dialog'
import { ConfirmHost, confirmDialog } from '../confirm'
import { LogConfirm } from './LogConfirm'
import { RadioPicker } from './RadioPicker'
import { BandMenu } from './BandMenu'
import type { LoggedQso } from '../types'
import type { RadioLaunchInfo } from '../api'

vi.setConfig({ testTimeout: 15_000 })

const W = 700
const H = 800
const STOP_BOX = { left: 294, top: 59, width: 75, height: 32 }
const OVERLAY = '.ui-dialog-overlay, .logconfirm-backdrop, .radio-picker-overlay, [role="menu"]'

/** The geometry jsdom lacks: Stop TX's box, the window for every overlay, nothing for anything else. */
function layout() {
  const rect = (l: number, t: number, w: number, h: number) =>
    ({ left: l, top: t, width: w, height: h, right: l + w, bottom: t + h, x: l, y: t, toJSON: () => ({}) }) as DOMRect
  vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (this: Element) {
    if (this.matches('[data-stop-control], [data-stop-mirror]')) return rect(STOP_BOX.left, STOP_BOX.top, STOP_BOX.width, STOP_BOX.height)
    if (this.matches(OVERLAY) || this.matches('[role="dialog"], [aria-modal="true"]')) return rect(0, 0, W, H)
    return rect(0, 0, 0, 0)
  })
  Object.defineProperty(HTMLElement.prototype, 'offsetWidth', { configurable: true, get() { return STOP_BOX.width } })
  // Topmost first: the last-drawn overlay over the point, then the stop control under it. Under a page whose pointer
  // events are locked, nothing outside the overlay's own layer is hit at all.
  document.elementsFromPoint = (x: number, y: number) => {
    const inBox = (el: Element) => {
      const r = el.getBoundingClientRect()
      return x >= r.left && x <= r.right && y >= r.top && y <= r.bottom && r.width > 0
    }
    const overlays = [...document.querySelectorAll(OVERLAY)].filter(inBox).reverse()
    const stops = [...document.querySelectorAll('[data-stop-control]')].filter(inBox).filter((e) => !inert(e))
    return [...overlays, ...stops, document.documentElement]
  }
}

/** True when `el` takes no pointer events: the nearest element up from it that sets `pointer-events` sets `none`. */
function inert(el: Element): boolean {
  for (let n: Element | null = el; n; n = n.parentElement) {
    const pe = (n as HTMLElement).style?.pointerEvents
    if (pe) return pe === 'none'
  }
  return false
}

/** One left click where Stop TX is drawn, sent where a browser sends it (see the header). */
function clickWhereDrawn(): void {
  const mirror = document.querySelector<HTMLElement>('.stop-layer [data-stop-mirror]')
  const x = STOP_BOX.left + STOP_BOX.width / 2
  const y = STOP_BOX.top + STOP_BOX.height / 2
  const under = document.elementsFromPoint(x, y)[0] as HTMLElement
  const hit = mirror ?? (inert(under) ? document.documentElement : under)
  fireEvent.pointerDown(hit, { button: 0, ctrlKey: false, pointerType: 'mouse' })
  fireEvent.mouseDown(hit, { button: 0 })
  fireEvent.pointerUp(hit, { button: 0, pointerType: 'mouse' })
  fireEvent.mouseUp(hit, { button: 0 })
  fireEvent.click(hit, { button: 0 })
}

/** A few frames: the layer follows the page on animation frames. */
const frames = () => act(() => new Promise<void>((r) => setTimeout(r, 80)))

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  Element.prototype.hasPointerCapture = () => false
  Element.prototype.setPointerCapture = () => {}
  Element.prototype.releasePointerCapture = () => {}
  Element.prototype.scrollIntoView = () => {}
})
beforeEach(layout)
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
  document.body.style.pointerEvents = ''
})

/** Stop TX as the top bar draws it, the stop layer, and `overlay` over them. */
function page(onHalt: () => void, overlay: ReactNode) {
  return render(
    <>
      <div className="app">
        <button type="button" className="op-btn stop" data-stop-control onClick={onHalt}>
          Stop TX
        </button>
      </div>
      {overlay}
      <StopLayer />
    </>,
  )
}

function OpenDialog() {
  const [open, setOpen] = useState(true)
  return (
    <Dialog open={open} onOpenChange={setOpen} title="Getting started">
      <input aria-label="inside" autoFocus />
    </Dialog>
  )
}

const QSO = { call: 'IA5X', grid: 'JN52', band: '20m', mode: 'FT8', rstSent: '-12', rstRcvd: '-09', comment: null } as unknown as LoggedQso
const PICKER: RadioLaunchInfo = {
  showPicker: true,
  radios: [{ id: 0, name: 'HF', inUse: false }, { id: 1, name: 'VHF', inUse: false }],
  autoChoice: 'windows',
  autoSeconds: 0,
} as unknown as RadioLaunchInfo

/** Each overlay: how to open it, and how to tell it is still open. */
const KINDS: { name: string; open: () => ReactNode; after?: () => Promise<void>; isOpen: () => boolean }[] = [
  {
    name: 'a ui/Dialog (Radix: the page locked, the keyboard trapped)',
    open: () => <OpenDialog />,
    isOpen: () => screen.queryByRole('dialog', { hidden: true }) != null,
  },
  {
    name: 'a ConfirmHost question',
    open: () => <ConfirmHost />,
    after: async () => {
      act(() => void confirmDialog({ title: 'Remove radio?', confirmLabel: 'Remove radio' }))
    },
    isOpen: () => screen.queryByText('Remove radio?') != null,
  },
  {
    name: 'the prompt to log (a full-window scrim)',
    open: () => <LogConfirm record={QSO} onConfirm={() => {}} onDiscard={() => {}} />,
    isOpen: () => document.querySelector('.logconfirm-backdrop') != null,
  },
  {
    name: 'the launch radio picker (z 3000)',
    open: () => <RadioPicker info={PICKER} onChoose={() => {}} onSingleRadio={() => {}} onTimeout={() => {}} error={null} />,
    isOpen: () => document.querySelector('.radio-picker-overlay') != null,
  },
  {
    name: "an aria-modal scrim of the Logbook's kind",
    open: () => (
      <div className="logconfirm-backdrop" role="dialog" aria-modal="true" aria-label="Erase the log?">
        <div className="logconfirm">Erase the log?</div>
      </div>
    ),
    isOpen: () => document.querySelector('.logconfirm-backdrop') != null,
  },
  {
    name: 'an open menu drawn over Stop TX (the band pill at xs)',
    open: () => (
      <BandMenu items={[{ value: '20m', label: '20m', conditionBand: '20m' }]} value="20m" onPick={() => {}}
        triggerLabel="20m" triggerClassName="freq-channel" ariaLabel="Band" />
    ),
    after: async () => {
      fireEvent.pointerDown(screen.getByRole('button', { name: 'Band' }), { button: 0, ctrlKey: false, pointerType: 'mouse' })
    },
    isOpen: () => screen.queryByRole('menu', { hidden: true }) != null,
  },
]

describe('one click where Stop TX is drawn stops once, and the overlay stays open', () => {
  for (const k of KINDS) {
    it(k.name, async () => {
      const halt = vi.fn()
      page(halt, k.open())
      await k.after?.()
      await frames()
      expect(k.isOpen(), 'premise: the overlay is open').toBe(true)
      const before = document.activeElement
      clickWhereDrawn()
      await frames()
      expect(halt).toHaveBeenCalledTimes(1)
      expect(k.isOpen(), 'the overlay is still open').toBe(true)
      expect(document.activeElement, 'the keyboard stays where it was').toBe(before)
    })
  }

  it('a mirror stands only while an overlay covers Stop TX, and a disabled stop is a disabled mirror', async () => {
    const halt = vi.fn()
    const { rerender } = page(halt, null)
    await frames()
    expect(document.querySelector('[data-stop-mirror]'), 'no overlay, no mirror').toBeNull()
    rerender(
      <>
        <div className="app">
          <button type="button" className="op-btn stop" data-stop-control disabled onClick={halt}>Stop TX</button>
        </div>
        <OpenDialog />
        <StopLayer />
      </>,
    )
    await frames()
    const mirror = document.querySelector<HTMLButtonElement>('[data-stop-mirror]')
    expect(mirror?.disabled).toBe(true)
    expect(mirror?.getAttribute('tabindex')).toBe('-1')
    expect(mirror?.closest('[aria-hidden="true"]')).not.toBeNull()
  })

  // The instrument's control: without the layer, the same click on the same page stops nothing.
  it('without the stop layer the click stops nothing (the control)', async () => {
    const halt = vi.fn()
    render(
      <>
        <div className="app">
          <button type="button" className="op-btn stop" data-stop-control onClick={halt}>Stop TX</button>
        </div>
        <OpenDialog />
      </>,
    )
    await frames()
    clickWhereDrawn()
    await frames()
    expect(halt).not.toHaveBeenCalled()
  })
})
