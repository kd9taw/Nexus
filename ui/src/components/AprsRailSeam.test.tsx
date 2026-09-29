// @vitest-environment jsdom
//
// APRS's station list width divider (layout L7): what a key, a drag and a reset do, what is
// announced, and what a stored width paints. jsdom lays nothing out, so each box's width is stubbed
// from a data attribute the host renders (`data-w`) and a resize is fired by hand. The real layout —
// the sheet clamping the width to 260 px … half the body — is measured in Chrome by the layout
// harness and computed in view-grids.test.ts.
import { useRef, useState } from 'react'
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'
import { AprsRailSeam } from './AprsRailSeam'
import { aprsRailValue } from '../features/aprsRail'

let observers: Array<() => void> = []
const resized = () => act(() => observers.forEach((f) => f()))
const realRect = HTMLElement.prototype.getBoundingClientRect

beforeEach(() => {
  observers = []
  globalThis.ResizeObserver = class {
    cb: () => void
    constructor(cb: () => void) {
      this.cb = cb
      observers.push(cb)
    }
    observe() {}
    unobserve() {}
    disconnect() {
      observers = observers.filter((f) => f !== this.cb)
    }
  } as unknown as typeof ResizeObserver
  HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
    const w = Number(this.dataset.w ?? 0)
    return { left: 0, right: w, width: w, top: 0, bottom: 600, height: 600, x: 0, y: 0, toJSON: () => ({}) } as DOMRect
  }
})
afterEach(() => {
  cleanup()
  HTMLElement.prototype.getBoundingClientRect = realRect
})

/** A body `bodyW` wide with its rail `railW` wide; `commits` records every width stored. */
function Host({ bodyW, railW, mapLeft = false, initial = null, commits }: { bodyW: number; railW: number; mapLeft?: boolean; initial?: number | null; commits: Array<number | null> }) {
  const body = useRef<HTMLDivElement>(null)
  const rail = useRef<HTMLDivElement>(null)
  const [stored, setStored] = useState<number | null>(initial)
  return (
    <div
      className="aprs-body"
      ref={body}
      data-w={bodyW}
      style={stored == null ? undefined : ({ '--aprs-rail-w': aprsRailValue(stored) } as React.CSSProperties)}
    >
      <div className="aprs-rail" ref={rail} data-w={railW} />
      <div className="aprs-map" />
      <AprsRailSeam
        body={body}
        rail={rail}
        stored={stored}
        mapLeft={mapLeft}
        onCommit={(px) => {
          commits.push(px)
          setStored(px)
        }}
        label="Station list column width"
      />
    </div>
  )
}

const sep = () => screen.getByRole('separator', { name: 'Station list column width' })
const body = () => document.querySelector<HTMLElement>('.aprs-body')!
const aria = (el: HTMLElement) => ['aria-valuenow', 'aria-valuemin', 'aria-valuemax'].map((a) => Number(el.getAttribute(a)))

describe('the APRS station list width divider', () => {
  it('is a focusable vertical separator announcing the rail’s width, from its 260 px floor to half the body', () => {
    render(<Host bodyW={1400} railW={420} commits={[]} />)
    const s = sep()
    expect(s.tabIndex).toBe(0)
    expect(s.getAttribute('aria-orientation')).toBe('vertical')
    expect(aria(s)).toEqual([420, 260, 700])
    expect(s.className).toContain('pane-splitter aprs-railseam')
  })

  it('on a body under twice the stock width still announces the stock width inside its range', () => {
    render(<Host bodyW={700} railW={420} commits={[]} />)
    expect(aria(sep())).toEqual([420, 260, 420])
  })

  it('answers the arrows the way they point (right widens the rail on the left), Shift for a big step, Home/End to its ends, Backspace to the stock width', () => {
    const commits: Array<number | null> = []
    render(<Host bodyW={1400} railW={420} commits={commits} />)
    const s = sep()
    fireEvent.keyDown(s, { key: 'ArrowRight' })
    expect(commits).toEqual([436])
    expect(body().style.getPropertyValue('--aprs-rail-w')).toBe('min(436px, max(50%, 420px))')
    expect(Number(s.getAttribute('aria-valuenow')), 'the next key must step from the committed width').toBe(436)
    fireEvent.keyDown(s, { key: 'ArrowLeft', shiftKey: true })
    expect(commits[commits.length - 1]).toBe(372)
    fireEvent.keyDown(s, { key: 'Home' })
    expect(commits[commits.length - 1]).toBe(260)
    fireEvent.keyDown(s, { key: 'End' })
    expect(commits[commits.length - 1]).toBe(700)
    fireEvent.keyDown(s, { key: 'Backspace' })
    expect(commits[commits.length - 1], 'a reset stores no width of its own').toBeNull()
    expect(body().style.getPropertyValue('--aprs-rail-w'), 'the stock width is no token at all').toBe('')
    // A chord belongs to the OS and the app, never the divider.
    const n = commits.length
    fireEvent.keyDown(s, { key: 'ArrowRight', ctrlKey: true })
    expect(commits.length).toBe(n)
  })

  it('with the map on the left the rail is on the right, and the arrow pointing away from it widens it', () => {
    const commits: Array<number | null> = []
    render(<Host bodyW={1400} railW={420} mapLeft commits={commits} />)
    fireEvent.keyDown(sep(), { key: 'ArrowLeft' })
    expect(commits).toEqual([436])
    fireEvent.keyDown(sep(), { key: 'ArrowRight' })
    expect(commits[commits.length - 1]).toBe(420)
  })

  it('never commits past its ends, whatever the key asks', () => {
    const commits: Array<number | null> = []
    render(<Host bodyW={1400} railW={690} initial={690} commits={commits} />)
    fireEvent.keyDown(sep(), { key: 'ArrowRight', shiftKey: true })
    expect(commits).toEqual([700])
  })

  it('a drag paints the width live and commits ONCE, on release; a click commits nothing', () => {
    const commits: Array<number | null> = []
    render(<Host bodyW={1400} railW={420} commits={commits} />)
    const s = sep()
    fireEvent.pointerDown(s, { button: 0, clientX: 426, pointerId: 1 })
    fireEvent.pointerMove(window, { clientX: 476, pointerId: 1 })
    expect(body().style.getPropertyValue('--aprs-rail-w'), 'painted live').toBe('min(470px, max(50%, 420px))')
    expect(commits, 'nothing stored mid-drag').toEqual([])
    fireEvent.pointerMove(window, { clientX: 506, pointerId: 1 })
    fireEvent.pointerUp(window, { clientX: 506, pointerId: 1 })
    expect(commits).toEqual([500])
    // A click: down and up with no move.
    fireEvent.pointerDown(s, { button: 0, clientX: 506, pointerId: 2 })
    fireEvent.pointerUp(window, { clientX: 506, pointerId: 2 })
    expect(commits).toEqual([500])
  })

  it('a cancelled drag puts back exactly what was painted before it: the stored width, or no token', () => {
    const commits: Array<number | null> = []
    const { unmount } = render(<Host bodyW={1400} railW={420} commits={commits} />)
    fireEvent.pointerDown(sep(), { button: 0, clientX: 426, pointerId: 1 })
    fireEvent.pointerMove(window, { clientX: 526, pointerId: 1 })
    fireEvent.pointerCancel(window, { pointerId: 1 })
    expect(body().style.getPropertyValue('--aprs-rail-w')).toBe('')
    unmount()
    render(<Host bodyW={1400} railW={500} initial={500} commits={commits} />)
    fireEvent.pointerDown(sep(), { button: 0, clientX: 506, pointerId: 1 })
    fireEvent.pointerMove(window, { clientX: 606, pointerId: 1 })
    fireEvent.pointerCancel(window, { pointerId: 1 })
    expect(body().style.getPropertyValue('--aprs-rail-w')).toBe('min(500px, max(50%, 420px))')
    expect(commits).toEqual([])
  })

  it('re-measures on every resize: a narrower body narrows the range, and what is announced follows the rail', () => {
    render(<Host bodyW={1400} railW={600} initial={600} commits={[]} />)
    expect(aria(sep())).toEqual([600, 260, 700])
    // The window shrinks: the layout clamps the rail to half the body (the sheet does it), and the
    // divider says so without a commit of its own.
    body().dataset.w = '1000'
    document.querySelector<HTMLElement>('.aprs-rail')!.dataset.w = '500'
    resized()
    expect(aria(sep())).toEqual([500, 260, 500])
  })

  it('announces nothing while the body is not laid out (a hidden keep-alive host), and a key then changes nothing', () => {
    const commits: Array<number | null> = []
    render(<Host bodyW={0} railW={0} commits={commits} />)
    const s = sep()
    expect(s.hasAttribute('aria-valuenow')).toBe(false)
    fireEvent.keyDown(s, { key: 'ArrowRight' })
    expect(commits).toEqual([])
  })
})
