// @vitest-environment jsdom
//
// THE CONTENT SIGNAL. Every pin reads layout (scrollHeight, or the rows' rects): run after every
// render, that is a forced synchronous layout per render of a feed's host — for Band Activity,
// re-rendered by every 4 Hz snapshot, a steady storm of them on an unchanged list. A caller that
// passes `content` gets the pin only when the content (or the edge, or the pin) changes.
//
// What must NOT change with it — the whole point of the hook:
//   • pinned follows the newest content, at either edge;
//   • scrolled away to read, new content never yanks the view, and at the top edge the row being
//     read holds still;
//   • repin() follows again;
//   • a keep-alive host (display:none) shown again re-pins (assessment V17) — here through the
//     scroller's resize, since an unrelated render no longer runs the pin.
//
// jsdom has no layout engine and no ResizeObserver: geometry is stubbed per element and the
// observer is a fake the test fires by hand.
import { describe, it, expect, afterEach, beforeEach } from 'vitest'
import { useState } from 'react'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { usePinnedScroll } from './usePinnedScroll'

/** A fake ResizeObserver: records what it watches; `fireResize()` delivers to every live one. */
const observers = new Set<{ cb: ResizeObserverCallback; els: Set<Element>; self: ResizeObserver }>()
class FakeResizeObserver {
  private rec: { cb: ResizeObserverCallback; els: Set<Element>; self: ResizeObserver }
  constructor(cb: ResizeObserverCallback) {
    this.rec = { cb, els: new Set(), self: this as unknown as ResizeObserver }
    observers.add(this.rec)
  }
  observe(el: Element) {
    this.rec.els.add(el)
  }
  unobserve(el: Element) {
    this.rec.els.delete(el)
  }
  disconnect() {
    observers.delete(this.rec)
  }
}
function fireResize() {
  for (const o of observers) if (o.els.size > 0) o.cb([], o.self)
}
const realRO = globalThis.ResizeObserver
beforeEach(() => {
  ;(globalThis as { ResizeObserver?: unknown }).ResizeObserver = FakeResizeObserver
})
afterEach(() => {
  cleanup()
  observers.clear()
  ;(globalThis as { ResizeObserver?: unknown }).ResizeObserver = realRO
  layout.clear()
})

/** Row top positions in content coordinates (top-edge tests). */
const layout = new Map<string, number>()

function Feed({ rows, edge = 'bottom' }: { rows: string[]; edge?: 'bottom' | 'top' }) {
  // The content signal is the row array itself, as OperateDecodes passes its memoised `drawn`.
  const { ref, pinned, onScroll, repin } = usePinnedScroll<HTMLDivElement>(edge, rows)
  const [, force] = useState(0)
  return (
    <div>
      <span data-testid="state">{pinned ? 'pinned' : 'reviewing'}</span>
      {/* A content-free re-render: a snapshot poll that changed nothing. */}
      <button onClick={() => force((n) => n + 1)}>render</button>
      <button onClick={repin}>repin</button>
      <div data-testid="feed" ref={ref} onScroll={onScroll}>
        {rows.map((r) => (
          <div key={r} data-pin-key={r} />
        ))}
      </div>
    </div>
  )
}

/** Stub geometry and COUNT every layout read the pin makes. */
function stub(feed: HTMLElement, geo: { scrollHeight: number; clientHeight: number }) {
  const reads = { n: 0 }
  Object.defineProperty(feed, 'scrollHeight', {
    configurable: true,
    get: () => {
      reads.n++
      return geo.scrollHeight
    },
  })
  Object.defineProperty(feed, 'clientHeight', { configurable: true, get: () => geo.clientHeight })
  feed.getBoundingClientRect = () => {
    reads.n++
    return { top: 0, bottom: geo.clientHeight, height: geo.clientHeight } as DOMRect
  }
  const wireRows = () => {
    for (const row of Array.from(feed.querySelectorAll<HTMLElement>('[data-pin-key]'))) {
      const key = row.getAttribute('data-pin-key')!
      row.getBoundingClientRect = () => {
        reads.n++
        const top = (layout.get(key) ?? 0) - feed.scrollTop
        return { top, bottom: top + 20, height: 20 } as DOMRect
      }
    }
  }
  wireRows()
  return { geo, reads, wireRows }
}

function place(rows: string[]) {
  layout.clear()
  rows.forEach((r, i) => layout.set(r, i * 20))
}

const state = () => screen.getByTestId('state').textContent
const feedEl = () => screen.getByTestId('feed')
const rerenderNoChange = () => fireEvent.click(screen.getByText('render'))

function mountBottom(rows: string[]) {
  const view = render(<Feed rows={rows} />)
  const s = stub(feedEl(), { scrollHeight: 1000, clientHeight: 100 })
  // The mount-time pin ran before the stub; a resize (the host laid out) brings it to the bottom.
  fireResize()
  return { ...view, ...s }
}

describe('usePinnedScroll with a content signal — bottom edge', () => {
  it('reads NO layout on a render whose content did not change', () => {
    const { reads } = mountBottom(['a', 'b'])
    expect(feedEl().scrollTop).toBe(1000)
    reads.n = 0
    rerenderNoChange()
    rerenderNoChange()
    rerenderNoChange()
    expect(reads.n).toBe(0)
  })

  it('pinned: new content follows to the newest at the bottom', () => {
    const { rerender, geo } = mountBottom(['a', 'b'])
    geo.scrollHeight = 1200
    rerender(<Feed rows={['a', 'b', 'c']} />)
    expect(feedEl().scrollTop).toBe(1200)
  })

  it('reading: new content never yanks the view, and repin() follows again', () => {
    const { rerender, geo } = mountBottom(['a', 'b'])
    const feed = feedEl()
    feed.scrollTop = 700
    fireEvent.scroll(feed)
    expect(state()).toBe('reviewing')
    geo.scrollHeight = 1200
    rerender(<Feed rows={['a', 'b', 'c']} />)
    expect(feed.scrollTop).toBe(700)
    fireEvent.click(screen.getByText('repin'))
    expect(state()).toBe('pinned')
    expect(feed.scrollTop).toBe(1200)
  })

  it('a keep-alive host shown again re-pins with NO content change (V17), through its resize', () => {
    mountBottom(['a', 'b'])
    const feed = feedEl()
    // display:none round trip: the offset is lost, no scroll event, nothing re-rendered.
    feed.scrollTop = 0
    rerenderNoChange()
    expect(feed.scrollTop).toBe(0) // an unrelated render no longer runs the pin…
    fireResize() // …the host coming back (0×0 → its real box) does
    expect(feed.scrollTop).toBe(1000)
  })

  it('a resize while reading leaves the view where the operator put it', () => {
    mountBottom(['a', 'b'])
    const feed = feedEl()
    feed.scrollTop = 700
    fireEvent.scroll(feed)
    fireResize()
    expect(feed.scrollTop).toBe(700)
  })

  it('stops watching when the feed unmounts', () => {
    const { unmount } = mountBottom(['a'])
    expect(observers.size).toBe(1)
    unmount()
    expect(observers.size).toBe(0)
  })
})

describe('usePinnedScroll with a content signal — top edge (newest on top)', () => {
  function mountTop(rows: string[]) {
    place(rows)
    const view = render(<Feed rows={rows} edge="top" />)
    const s = stub(feedEl(), { scrollHeight: rows.length * 20, clientHeight: 100 })
    fireResize()
    return { ...view, ...s }
  }

  it('reads NO layout on an unchanged render, pinned or reading', () => {
    const { reads } = mountTop(['r5', 'r4', 'r3', 'r2', 'r1', 'r0', 'q9', 'q8'])
    const feed = feedEl()
    reads.n = 0
    rerenderNoChange()
    expect(reads.n).toBe(0)
    feed.scrollTop = 60
    fireEvent.scroll(feed)
    expect(state()).toBe('reviewing')
    reads.n = 0
    rerenderNoChange()
    rerenderNoChange()
    expect(reads.n).toBe(0)
  })

  it('pinned: new rows on top keep the view at the top', () => {
    const { rerender } = mountTop(['r5', 'r4', 'r3', 'r2', 'r1', 'r0'])
    const feed = feedEl()
    feed.scrollTop = 17 // a layout nudge with no scroll event
    rerender(<Feed rows={['s0', 'r5', 'r4', 'r3', 'r2', 'r1', 'r0']} edge="top" />)
    expect(feed.scrollTop).toBe(0)
  })

  it('reading: rows arriving above hold the row being read still, bottom trimmed or not', () => {
    const first = ['r5', 'r4', 'r3', 'r2', 'r1', 'r0', 'q9', 'q8', 'q7', 'q6']
    const { rerender, geo, wireRows } = mountTop(first)
    const feed = feedEl()
    feed.scrollTop = 60 // reading r2
    fireEvent.scroll(feed)
    const next = ['s1', 's0', 'r5', 'r4', 'r3', 'r2', 'r1', 'r0', 'q9', 'q8']
    place(next)
    geo.scrollHeight = next.length * 20
    // The two new rows have no stub yet and the pin runs inside the render: give every pin-keyed
    // row its layout through the prototype for the duration of it.
    const proto = HTMLElement.prototype.getBoundingClientRect
    HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
      const key = this.getAttribute('data-pin-key')
      if (key == null) return proto.call(this)
      const top = (layout.get(key) ?? 0) - feed.scrollTop
      return { top, bottom: top + 20, height: 20 } as DOMRect
    }
    try {
      rerender(<Feed rows={next} edge="top" />)
    } finally {
      HTMLElement.prototype.getBoundingClientRect = proto
    }
    wireRows()
    expect(feed.scrollTop).toBe(100) // r2 is still at the top of the view
  })

  it('flipping the edge re-pins at the new edge', () => {
    const rows = ['a', 'b', 'c']
    const { rerender, geo } = mountBottom(rows)
    const feed = feedEl()
    expect(feed.scrollTop).toBe(1000)
    geo.scrollHeight = 1000
    rerender(<Feed rows={rows} edge="top" />)
    expect(feed.scrollTop).toBe(0)
  })
})
