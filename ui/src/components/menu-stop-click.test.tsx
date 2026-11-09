// @vitest-environment jsdom
//
// STOP TX WORKS ON THE FIRST CLICK WHILE A MENU IS OPEN.
//
// A Radix menu is modal by default, and a modal menu puts `pointer-events: none` on <body> while it is open:
// in a browser, the one click on Stop TX is hit-tested past the button, lands on <html>, closes the menu and
// stops nothing. Measured in real Chrome with the Help menu open: halts +0; not modal: halts +1. Six of the
// app's twelve menus were modal, so every one now opens through ui/MenuParts.tsx, and this file holds them to it.
//
// HOW A CLICK IS MADE HERE. `fireEvent.click(stop)` would run Stop TX's handler whatever lay over it: jsdom
// neither hit-tests nor honours `pointer-events`. So `clickAsChrome` does what Chrome does with a left click,
// for the one property that decides this: an element under `pointer-events: none` (its own, or inherited
// from the nearest ancestor that sets it) is not hit, and the press and its click go to <html> instead.
// Geometry (something drawn OVER Stop TX) is jsdom-blind and is the real-Chrome probe's matter
// (`scripts/browser-probe --desktop menustop`).
//
// What is asserted, per menu: the menu opens, ONE click on Stop TX runs the stop handler exactly once, and
// the menu is closed by it. Then the census: every Radix menu root and content in ui/src is MenuParts', and
// no other Radix menu-like package (Popover, Menubar, ContextMenu, Select) is used without the same rule.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen } from '@testing-library/react'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'
import { useState, type ReactNode } from 'react'
import { TopBar } from './TopBar'
import { BandMenu } from './BandMenu'
import { AtuSwitchOutMenu } from './AtuSwitchOutMenu'
import { CallRowMenu } from './CallRowMenu'
import { Js8ListMenu } from './Js8ListMenu'
import { BoxMenu } from './connect/BoxMenu'
import { Menu } from './ui/Menu'
import type { RadioStatus } from '../types'

vi.setConfig({ testTimeout: 15_000 })

beforeAll(() => {
  // Radix DropdownMenu measures and captures pointers; jsdom has neither.
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

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

/** True when `el` takes no pointer events: the nearest element up from it that sets `pointer-events` sets `none`. */
function inert(el: Element): boolean {
  for (let n: Element | null = el; n; n = n.parentElement) {
    const pe = (n as HTMLElement).style?.pointerEvents
    if (pe) return pe === 'none'
  }
  return false
}

/** One left click at `el`, hit-tested as Chrome does for `pointer-events` (see the header). */
function clickAsChrome(el: HTMLElement): void {
  const hit = inert(el) ? document.documentElement : el
  fireEvent.pointerDown(hit, { button: 0, ctrlKey: false, pointerType: 'mouse' })
  fireEvent.mouseDown(hit, { button: 0 })
  fireEvent.pointerUp(hit, { button: 0, pointerType: 'mouse' })
  fireEvent.mouseUp(hit, { button: 0 })
  fireEvent.click(hit, { button: 0 })
}

/** Radix opens a menu on the trigger's pointerdown, and listens for a press outside it from the next tick. */
async function openWith(trigger: HTMLElement): Promise<void> {
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false, pointerType: 'mouse' })
  await settle()
}
const settle = () => act(() => new Promise<void>((r) => setTimeout(r, 0)))

/** A Stop TX button beside the menu, as the cockpit's stands beside its header's menus. */
function withStop(menu: ReactNode, onHalt: () => void) {
  return render(
    <div className="app">
      <button type="button" className="op-btn stop" onClick={onHalt}>
        Stop TX
      </button>
      {menu}
    </div>,
  )
}

/** The ATU menu is opened by its button's right-click or long press, so the test holds it open. */
function AtuOpen() {
  const [open, setOpen] = useState(true)
  return (
    <button type="button">
      ATU
      <AtuSwitchOutMenu open={open} onOpenChange={setOpen} onSwitchOut={() => {}} buttonRef={{ current: null }} />
    </button>
  )
}

const radio = {
  dialMhz: 14.074, band: '20m', sideband: 'USB', rigMode: 'USB', rigConfirmed: true, nextSlotMs: 0, txEven: true,
  txCycleAuto: true, txEnabled: true, txAllowed: true, transmitting: true, tuning: false, qsoRecording: false,
  catOk: true, dtSec: 0, clockOffsetMs: 0,
} as unknown as RadioStatus

/** A row's menu is open for as long as its host renders it; closing asks the host to stop. */
function CallRowOpen() {
  const [open, setOpen] = useState(true)
  return open ? <CallRowMenu call="EA1ABC" band="20m" country="Spain" onClose={() => setOpen(false)} /> : null
}

/** The real top bar where it carries the TX cluster (the Logbook, the map): its Stop TX is the one clicked. */
function renderBar(onHaltTx: () => void) {
  const noop = () => {}
  return render(
    <div className="app">
      <TopBar
        mycall="KD9TAW" mygrid="EN52xa" radio={radio} link={{ tier: 'FT8' } as never} bandPlan={[]}
        onSetFrequency={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={onHaltTx}
        onSetTxEven={noop} onSetTxCycleAuto={noop} onSetHoldTxFreq={noop}
        tier="FT8" onTierChange={noop} onOpenGuide={noop} onOpenWalk={noop}
      />
    </div>,
  )
}

/** Each menu: how to mount it beside Stop TX, and how to open it. */
const MENUS: { name: string; mount: (onHalt: () => void) => void; open: () => Promise<void> }[] = [
  {
    name: 'Help (ui/Menu, in the real top bar, against its own Stop TX)',
    mount: (onHalt) => renderBar(onHalt),
    open: () => openWith(screen.getByRole('button', { name: 'Help' })),
  },
  {
    name: 'ui/Menu',
    mount: (onHalt) => withStop(<Menu trigger={<button type="button">More</button>} items={[{ label: 'One', onSelect: () => {} }]} />, onHalt),
    open: () => openWith(screen.getByRole('button', { name: 'More' })),
  },
  {
    name: 'BandMenu (the band/channel pill)',
    mount: (onHalt) =>
      withStop(
        <BandMenu
          items={[{ value: '20m', label: '20m', conditionBand: '20m' }, { value: '40m', label: '40m', conditionBand: '40m' }]}
          value="20m" onPick={() => {}} triggerLabel="20m" triggerClassName="freq-channel" ariaLabel="Band"
        />,
        onHalt,
      ),
    open: () => openWith(screen.getByRole('button', { name: 'Band' })),
  },
  {
    name: 'AtuSwitchOutMenu (the ATU button)',
    mount: (onHalt) => withStop(<AtuOpen />, onHalt),
    open: settle,
  },
  {
    name: 'CallRowMenu (a decode or roster row)',
    mount: (onHalt) => withStop(<CallRowOpen />, onHalt),
    open: settle,
  },
  {
    name: 'Js8ListMenu (a JS8 pane ⋯)',
    mount: (onHalt) => withStop(<Js8ListMenu label="Activity menu" entries={() => [{ kind: 'item', label: 'Clear', onSelect: () => {} }]} />, onHalt),
    open: () => openWith(screen.getByRole('button', { name: 'Activity menu' })),
  },
  {
    name: 'BoxMenu (a Connect box ⋯)',
    mount: (onHalt) => withStop(<BoxMenu title="Spots" textScale={1} onTextScale={() => {}} />, onHalt),
    open: () => openWith(document.querySelector<HTMLElement>('button.pane-menu')!),
  },
]

describe('one click on Stop TX stops, whatever menu is open', () => {
  for (const m of MENUS) {
    it(m.name, async () => {
      const halt = vi.fn()
      m.mount(halt)
      await m.open()
      expect(screen.getByRole('menu')).toBeTruthy()
      // `hidden`: a modal menu also marks the page aria-hidden; the button is found either way, so what fails
      // below is the click, not the lookup.
      clickAsChrome(screen.getByRole('button', { name: 'Stop TX', hidden: true }))
      await settle()
      expect(halt).toHaveBeenCalledTimes(1)
      expect(screen.queryByRole('menu')).toBeNull()
    })
  }

  // The instrument's own control: a page whose pointer events are locked must swallow the click, or the
  // assertion above could not fail.
  it('a locked page swallows the click (the control)', () => {
    const halt = vi.fn()
    withStop(null, halt)
    document.body.style.pointerEvents = 'none'
    try {
      clickAsChrome(screen.getByRole('button', { name: 'Stop TX' }))
    } finally {
      document.body.style.pointerEvents = ''
    }
    expect(halt).not.toHaveBeenCalled()
  })
})

describe('the keyboard keeps every meaning it had', () => {
  it('Enter opens, arrows and typeahead move, Esc closes and gives the keyboard back without scrolling', async () => {
    renderBar(() => {})
    const help = screen.getByRole('button', { name: 'Help' })
    const focus = vi.spyOn(HTMLElement.prototype, 'focus')
    const scroll = vi.spyOn(Element.prototype, 'scrollIntoView')
    help.focus()
    fireEvent.keyDown(help, { key: 'Enter' })
    await settle()
    const menu = screen.getByRole('menu')
    const items = screen.getAllByRole('menuitem')
    expect(items.map((i) => i.textContent)).toEqual(['Getting started', 'Your first FT8 contact'])
    expect(document.activeElement).toBe(items[0])
    // Keys go where the browser sends them: to the focused item, bubbling through the menu.
    // Radix moves the roving focus a tick after the key.
    const press = async (key: string) => {
      fireEvent.keyDown(document.activeElement as HTMLElement, { key })
      await settle()
    }
    await press('ArrowDown')
    expect(document.activeElement).toBe(items[1])
    await press('G')
    expect(document.activeElement).toBe(items[0])
    expect(menu.contains(document.activeElement)).toBe(true)
    focus.mockClear()
    await press('Escape')
    expect(screen.queryByRole('menu')).toBeNull()
    expect(document.activeElement).toBe(help)
    const back = focus.mock.contexts.findIndex((c) => c === help)
    expect(back).toBeGreaterThanOrEqual(0)
    expect(focus.mock.calls[back]).toEqual([{ preventScroll: true }])
    expect(scroll.mock.contexts).toContain(help)
    expect(scroll.mock.calls[scroll.mock.contexts.indexOf(help)]).toEqual([{ block: 'nearest' }])
  })

  it('a click outside leaves the keyboard where the click put it, not on the menu button', async () => {
    renderBar(() => {})
    const help = screen.getByRole('button', { name: 'Help' })
    await openWith(help)
    const stop = screen.getByRole('button', { name: 'Stop TX', hidden: true })
    // jsdom moves no focus on a press; Chrome puts it on the pressed button, so this does.
    stop.focus()
    clickAsChrome(stop)
    await settle()
    expect(screen.queryByRole('menu')).toBeNull()
    expect(document.activeElement).toBe(stop)
  })
})

// ── the census ────────────────────────────────────────────────────────────────────────────────
const SRC = resolve(__dirname, '..')
const PARTS = join(SRC, 'components', 'ui', 'MenuParts.tsx')

function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name)
    if (statSync(p).isDirectory()) return sources(p)
    return /\.tsx?$/.test(name) && !/\.test\.tsx?$/.test(name) ? [p] : []
  })
}

describe('every Radix menu in ui/src opens through MenuParts', () => {
  const files = sources(SRC).map((p) => ({ p, src: readFileSync(p, 'utf8') }))

  it('uses no Radix Popover, Menubar, ContextMenu or Select (each would need the same not-modal rule)', () => {
    const found = files
      .filter(({ src }) => /from '@radix-ui\/react-(popover|menubar|context-menu|select)'/.test(src))
      .map(({ p }) => relative(SRC, p))
    expect(found).toEqual([])
  })

  it("renders no DropdownMenu Root or Content of its own: MenuParts' are not modal", () => {
    const users = files.filter(({ p, src }) => p !== PARTS && src.includes("'@radix-ui/react-dropdown-menu'"))
    // The census sees the twelve it was written against, so a broken file walk cannot pass it empty.
    expect(users.length).toBeGreaterThanOrEqual(12)
    const own = users.flatMap(({ p, src }) => {
      const alias = /import \* as (\w+) from '@radix-ui\/react-dropdown-menu'/.exec(src)?.[1]
      const named = /import \{[^}]*\b(Root|Content|DropdownMenu|DropdownMenuContent)\b[^}]*\} from '@radix-ui\/react-dropdown-menu'/.test(src)
      const bad = named || (alias != null && new RegExp(`<${alias}\\.(Root|Content)\\b`).test(src))
      return bad ? [relative(SRC, p)] : []
    })
    expect(own).toEqual([])
  })
})
