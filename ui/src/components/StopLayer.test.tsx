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
import { TopBar } from './TopBar'
import { useEscStop } from '../useEscStop'
import type { RadioStatus } from '../types'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'

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

  it("a dock's Esc Stop under a dialog: a click on its label stops it, the dialog open", async () => {
    const stop = vi.fn()
    render(
      <>
        <div className="app">
          <button type="button" className="cw-macro rtty-stop send-row-stop" data-stop-control onClick={stop}>
            <span className="cw-macro-key">Esc</span>
            <span className="cw-macro-label">Stop</span>
          </button>
        </div>
        <OpenDialog />
        <StopLayer />
      </>,
    )
    await frames()
    const label = document.querySelector<HTMLElement>('.stop-layer [data-stop-mirror] .cw-macro-label')!
    expect(label.textContent).toBe('Stop')
    fireEvent.pointerDown(label, { button: 0, pointerType: 'mouse' })
    fireEvent.mouseDown(label, { button: 0 })
    fireEvent.pointerUp(label, { button: 0, pointerType: 'mouse' })
    fireEvent.mouseUp(label, { button: 0 })
    fireEvent.click(label, { button: 0 })
    await frames()
    expect(stop).toHaveBeenCalledTimes(1)
    expect(screen.queryByRole('dialog', { hidden: true })).not.toBeNull()
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

// ── THE MIRROR IS A SECOND STOP TX, SO IT IS HELD TO THREE THINGS ───────────────────────────────────────────────
/** A microtask: the layer's MutationObserver answers in one, before the next frame is painted. */
const microtask = () => act(() => Promise.resolve())
const mirror = () => document.querySelector<HTMLButtonElement>('.stop-layer [data-stop-mirror]')

/** A full mouse press on `el`, as Chrome sends it. */
function press(el: HTMLElement) {
  fireEvent.pointerDown(el, { button: 0, pointerType: 'mouse' })
  fireEvent.mouseDown(el, { button: 0 })
  fireEvent.pointerUp(el, { button: 0, pointerType: 'mouse' })
  fireEvent.mouseUp(el, { button: 0 })
  fireEvent.click(el, { button: 0 })
}
/** A tap on `el`, as Chrome sends it: pointer and touch events, then the compatibility mouse events and the click. */
function tap(el: HTMLElement) {
  const at = { clientX: 300, clientY: 70 }
  fireEvent.pointerDown(el, { button: 0, pointerType: 'touch' })
  fireEvent.touchStart(el, { touches: [at], changedTouches: [at] })
  fireEvent.pointerUp(el, { button: 0, pointerType: 'touch' })
  fireEvent.touchEnd(el, { touches: [], changedTouches: [at] })
  fireEvent.mouseDown(el, { button: 0 })
  fireEvent.mouseUp(el, { button: 0 })
  fireEvent.click(el, { button: 0 })
}

function Stops({ disabled = false, hidden = false, shown = true, onHalt }: { disabled?: boolean; hidden?: boolean; shown?: boolean; onHalt: () => void }) {
  return (
    <>
      <div className="app" hidden={hidden}>
        {shown && (
          <button type="button" className="op-btn stop" data-stop-control disabled={disabled} onClick={onHalt}>
            Stop TX
          </button>
        )}
      </div>
      <OpenDialog />
      <StopLayer />
    </>
  )
}

/** Every place in ui/src's app source (tests, probe pages and the layer itself aside) that writes `data-stop-control`,
 *  as `file Component <element class="…"> = value`, in file order. Comments are not code and are skipped. */
function markerSites(): string[] {
  const SRC = resolve(__dirname, '..')
  const walk = (dir: string): string[] =>
    readdirSync(dir).flatMap((n) => {
      const p = join(dir, n)
      if (statSync(p).isDirectory()) return n === 'probe' ? [] : walk(p)
      return /\.tsx?$/.test(n) && !/\.test\.tsx?$/.test(n) && !/testkit/.test(n) && n !== 'StopLayer.tsx' ? [p] : []
    })
  const out: string[] = []
  for (const p of walk(SRC).sort()) {
    const code = readFileSync(p, 'utf8')
      .replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, ' '))
      .replace(/(^|\s)\/\/.*$/gm, '$1')
    for (const m of code.matchAll(/data-stop-control(?:=(?:"([^"]*)"|\{[^}]*\}))?/g)) {
      const at = m.index!
      const start = code.lastIndexOf('<', at)
      // The tag's end: the first `>` outside braces (an arrow inside `onClick={() => …}` is inside them).
      let end = start
      for (let depth = 0; end < code.length; end++) {
        const ch = code[end]
        if (ch === '{') depth++
        else if (ch === '}') depth--
        else if (ch === '>' && depth === 0) break
      }
      const tag = code.slice(start, end)
      const element = /^<([\w.]+)/.exec(tag)?.[1] ?? '?'
      const cls = /className=(?:"([^"]*)"|\{`([^`]*)`\})/.exec(tag)
      const comps = [...code.slice(0, at).matchAll(/^(?:export\s+)?(?:default\s+)?(?:function\s+([A-Z]\w*)|const\s+([A-Z]\w*)\s*=)/gm)]
      const last = comps[comps.length - 1]
      out.push(`${relative(SRC, p)} ${last?.[1] ?? last?.[2] ?? '?'} <${element} class="${cls?.[1] ?? cls?.[2] ?? ''}"> = ${m[1] ?? '(none)'}`)
    }
  }
  return out.sort()
}

describe('1. the mirror always matches its button, and never outlives it', () => {
  it('disabled, the mirror is disabled at once; enabled again, it is enabled at once (no frame waited)', async () => {
    const halt = vi.fn()
    const { rerender } = render(<Stops onHalt={halt} />)
    await frames()
    expect(mirror()?.disabled).toBe(false)
    rerender(<Stops onHalt={halt} disabled />)
    await microtask()
    expect(mirror()?.disabled).toBe(true)
    press(mirror()!)
    expect(halt, 'a press on a disabled stop stops nothing').not.toHaveBeenCalled()
    rerender(<Stops onHalt={halt} />)
    await microtask()
    expect(mirror()?.disabled).toBe(false)
  })

  it('unmounted, or put behind [hidden], the mirror is gone at once', async () => {
    const halt = vi.fn()
    const { rerender } = render(<Stops onHalt={halt} />)
    await frames()
    expect(mirror()).not.toBeNull()
    rerender(<Stops onHalt={halt} shown={false} />)
    await microtask()
    expect(mirror(), 'unmounted').toBeNull()
    rerender(<Stops onHalt={halt} />)
    await frames()
    expect(mirror()).not.toBeNull()
    rerender(<Stops onHalt={halt} hidden />)
    await microtask()
    expect(mirror(), 'behind [hidden]').toBeNull()
  })

  it('a press on a mirror whose button has just gone does nothing, and takes the mirror with it', async () => {
    const halt = vi.fn()
    const btn = document.createElement('button')
    btn.className = 'op-btn stop'
    btn.setAttribute('data-stop-control', '')
    btn.textContent = 'Stop TX'
    btn.addEventListener('click', halt)
    document.body.appendChild(btn)
    render(<><OpenDialog /><StopLayer /></>)
    await frames()
    const m = mirror()!
    expect(m).not.toBeNull()
    // Gone in the same task as the press: no observer has had its turn yet, so only the click's own check stands.
    btn.remove()
    press(m)
    expect(halt).not.toHaveBeenCalled()
    expect(m.isConnected).toBe(false)
  })

  it('a press in progress holds its mirror only until the press ends, then the mirror of a button that went is gone', async () => {
    const halt = vi.fn()
    const { rerender } = render(<Stops onHalt={halt} />)
    await frames()
    fireEvent.pointerDown(mirror()!, { button: 0, pointerType: 'mouse' })
    rerender(<Stops onHalt={halt} shown={false} />)
    await microtask()
    expect(mirror(), 'held under the pressed pointer').not.toBeNull()
    // The pointer is let go somewhere else: no click on the mirror, and the mirror goes with the press.
    fireEvent.pointerUp(window, { button: 0, pointerType: 'mouse' })
    await frames()
    expect(mirror()).toBeNull()
    expect(halt).not.toHaveBeenCalled()
  })
})

// The press that lands across a re-render. React may replace a button's node between the press and its click (a key
// change, a remount); the press must still stop — on the stop that took the button's place, matched by its marker
// identity at the press point — and never reach anything else.
describe('a press that lands across a re-render', () => {
  const at = { clientX: STOP_BOX.left + STOP_BOX.width / 2, clientY: STOP_BOX.top + STOP_BOX.height / 2 }
  function Swap({ which, onFirst, onSecond }: { which: 'first' | 'same' | 'unmarked' | 'other'; onFirst: () => void; onSecond: () => void }) {
    return (
      <>
        <div className="app">
          {which === 'first' && <button key="a" type="button" className="op-btn stop" data-stop-control="stop-tx" onClick={onFirst}>Stop TX</button>}
          {which === 'same' && <button key="b" type="button" className="op-btn stop" data-stop-control="stop-tx" onClick={onSecond}>Stop TX</button>}
          {which === 'unmarked' && <button key="c" type="button" className="op-btn stop" onClick={onSecond}>Stop TX</button>}
          {which === 'other' && <button key="d" type="button" className="cw-macro rtty-stop" data-stop-control="esc-stop" onClick={onSecond}>Esc Stop</button>}
        </div>
        <OpenDialog />
        <StopLayer />
      </>
    )
  }
  async function pressAcross(to: 'same' | 'unmarked' | 'other') {
    const first = vi.fn()
    const second = vi.fn()
    const { rerender } = render(<Swap which="first" onFirst={first} onSecond={second} />)
    await frames()
    const m = mirror()!
    fireEvent.pointerDown(m, { button: 0, pointerType: 'mouse', ...at })
    fireEvent.mouseDown(m, { button: 0, ...at })
    // The node is replaced between the press and its click.
    rerender(<Swap which={to} onFirst={first} onSecond={second} />)
    await microtask()
    fireEvent.pointerUp(m, { button: 0, pointerType: 'mouse', ...at })
    fireEvent.mouseUp(m, { button: 0, ...at })
    fireEvent.click(m, { button: 0, ...at })
    return { first, second }
  }

  it('replaced by the same stop: the press stops, once, on the stop that took its place', async () => {
    const { first, second } = await pressAcross('same')
    expect([first.mock.calls.length, second.mock.calls.length]).toEqual([0, 1])
  })
  it('replaced by an UNMARKED control: the press does nothing', async () => {
    const { first, second } = await pressAcross('unmarked')
    expect([first.mock.calls.length, second.mock.calls.length]).toEqual([0, 0])
  })
  it('replaced by a different stop (another identity): the press does nothing', async () => {
    const { first, second } = await pressAcross('other')
    expect([first.mock.calls.length, second.mock.calls.length]).toEqual([0, 0])
  })
})

describe('2. one press is one stop', () => {
  it('a mouse press, a tap: each stops exactly once, and the button under the mirror is never pressed too', async () => {
    const halt = vi.fn()
    render(<Stops onHalt={halt} />)
    await frames()
    press(mirror()!)
    expect(halt).toHaveBeenCalledTimes(1)
    await frames()
    tap(mirror()!)
    expect(halt).toHaveBeenCalledTimes(2)
  })

  it('takes no keyboard: not focusable, and Enter, Space and a held key are refused even if code focuses it', async () => {
    const halt = vi.fn()
    render(<Stops onHalt={halt} />)
    await frames()
    const m = mirror()!
    expect(m.getAttribute('tabindex')).toBe('-1')
    for (const key of ['Enter', ' ']) {
      expect(fireEvent.keyDown(m, { key }), `${JSON.stringify(key)} is refused`).toBe(false)
      expect(fireEvent.keyDown(m, { key, repeat: true }), `${JSON.stringify(key)} held is refused`).toBe(false)
    }
    expect(halt).not.toHaveBeenCalled()
  })
})

describe('3. only stop controls carry the marker', () => {
  const radio = {
    dialMhz: 14.074, band: '20m', sideband: 'USB', rigMode: 'USB', rigConfirmed: true, nextSlotMs: 0, txEven: true,
    txCycleAuto: true, txEnabled: true, txAllowed: true, transmitting: true, tuning: false, qsoRecording: false,
    catOk: true, dtSec: 0, clockOffsetMs: 0,
  } as unknown as RadioStatus

  it('in the top bar, only Stop TX is marked (TX On, Tune, Hold are not)', () => {
    const noop = () => {}
    render(
      <TopBar mycall="KD9TAW" mygrid="EN52xa" radio={radio} link={{ tier: 'FT8' } as never} bandPlan={[]}
        onSetFrequency={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={noop} onSetTxEven={noop}
        onSetTxCycleAuto={noop} onSetHoldTxFreq={noop} tier="FT8" onTierChange={noop} onOpenGuide={noop} />,
    )
    const marked = [...document.querySelectorAll('[data-stop-control]')].map((e) => e.textContent?.trim())
    expect(marked).toEqual(['Stop TX'])
    expect(screen.getByRole('button', { name: 'Tune' }).hasAttribute('data-stop-control')).toBe(false)
  })

  // THE ALLOW-LIST. The stop layer draws every marked control above every dialog, so a marker on anything that is
  // not a stop (Tune, PTT, the TX latch, ATU, RTTY's Auto CQ, a macro) would let it key from above a dialog. Every
  // place in the app's source that writes the marker is pinned here EXACTLY: by file, by the component it is in, by
  // the element and its class, and by the marker's value (the stop's identity, which the mirror's press uses to find
  // the same stop again). Any extra, missing, moved or renamed marker goes red. ONLY A CONTROL THAT STOPS A
  // TRANSMISSION MAY EVER BE ADDED.
  it('the marker is on exactly the eight stop controls, and nowhere else', () => {
    expect(markerSites()).toEqual([
      'components/CockpitTxStrip.tsx CockpitTxStrip <button class="op-btn stop"> = stop-tx',
      'components/FtStopControl.tsx FtStopControl <button class="op-btn stop"> = stop-tx',
      'components/JttyCockpit.tsx JttyCockpit <button class="cw-macro rtty-stop jtty-stop"> = esc-stop',
      'components/PskCockpit.tsx PskCockpit <button class="cw-macro rtty-stop psk-stop send-row-stop"> = esc-stop',
      'components/RttyCockpit.tsx RttyCockpit <button class="cw-macro rtty-auto-stop"> = sequencer-abort',
      'components/RttyCockpit.tsx RttyCockpit <button class="cw-macro rtty-stop send-row-stop"> = esc-stop',
      'components/SstvView.tsx SstvView <button class="sstv-tx-stop"> = sstv-stop',
      'components/TopBar.tsx TopBar <button class="op-btn stop"> = stop-tx',
    ])
  })
})

describe('screen readers and Esc', () => {
  it('the button stays the one accessible Stop TX; the layer and the mirror are aria-hidden', async () => {
    const halt = vi.fn()
    page(halt, (
      <div className="logconfirm-backdrop" role="dialog" aria-modal="true" aria-label="Erase the log?">
        <div className="logconfirm">Erase the log?</div>
      </div>
    ))
    await frames()
    const m = mirror()!
    expect(m).not.toBeNull()
    const named = screen.getAllByRole('button', { name: 'Stop TX' })
    expect(named).toHaveLength(1)
    expect(named[0].hasAttribute('data-stop-control')).toBe(true)
    expect(m.getAttribute('aria-hidden')).toBe('true')
    expect(m.closest('.stop-layer')?.getAttribute('aria-hidden')).toBe('true')
    expect(m.id).toBe('')
  })

  it('Esc still stops with a dialog and its mirror up, once', async () => {
    const stop = vi.fn()
    function EscStop() {
      useEscStop(true, stop)
      return null
    }
    render(<><EscStop /><Stops onHalt={() => {}} /></>)
    await frames()
    expect(mirror()).not.toBeNull()
    fireEvent.keyDown(screen.getByRole('textbox', { name: 'inside', hidden: true }), { key: 'Escape' })
    expect(stop).toHaveBeenCalledTimes(1)
  })
})
