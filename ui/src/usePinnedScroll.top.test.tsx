// @vitest-environment jsdom
//
// #276 — THE SAME PIN DISCIPLINE, MIRRORED: a newest-at-top feed pins to the TOP.
//
// Pinned = follow the newest rows, which now arrive at the top (scrollTop 0). Scrolled down past
// the slop = the operator is READING, and the view is never yanked — which for a top-growing feed
// means something the bottom mode never needed: rows land ABOVE the reader, so doing nothing would
// slide the row under their eyes down the screen. The hook holds the first visible row still
// (found by `data-pin-key`), and does so by the row, not by the height change, because the pane's
// render window trims the oldest rows off the BOTTOM in the same render (the height delta nets to
// zero at the cap while the row being read still moved).
//
// jsdom has no layout engine: scroll geometry and row rects are stubbed per element.
import { describe, it, expect, afterEach } from 'vitest'
import { useState } from 'react'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { usePinnedScroll, PIN_SLOP_PX } from './usePinnedScroll'

/** Row top positions in CONTENT coordinates (px from the top of the scrolled content). */
const layout = new Map<string, number>()

function Feed({ rows }: { rows: string[] }) {
  const { ref, pinned, onScroll, repin } = usePinnedScroll<HTMLDivElement>('top')
  const [, force] = useState(0)
  return (
    <div>
      <span data-testid="state">{pinned ? 'pinned' : 'reviewing'}</span>
      <button onClick={() => force((n) => n + 1)}>render</button>
      <button onClick={repin}>repin</button>
      <div data-testid="feed" ref={ref} onScroll={onScroll}>
        {rows.map((r) => (
          <div key={r} data-pin-key={r} data-testid={`row-${r}`} />
        ))}
      </div>
    </div>
  )
}

/** Wire the stubbed geometry: the container's rect is fixed at top 0; each row's rect top is its
 *  content position minus the container's live scrollTop — what a real browser reports. */
function stub(feed: HTMLElement, geo: { scrollHeight: number; clientHeight: number }) {
  Object.defineProperty(feed, 'scrollHeight', { configurable: true, get: () => geo.scrollHeight })
  Object.defineProperty(feed, 'clientHeight', { configurable: true, get: () => geo.clientHeight })
  feed.getBoundingClientRect = () => ({ top: 0, bottom: geo.clientHeight, height: geo.clientHeight }) as DOMRect
  const wireRows = () => {
    for (const row of Array.from(feed.querySelectorAll<HTMLElement>('[data-pin-key]'))) {
      const key = row.getAttribute('data-pin-key')!
      row.getBoundingClientRect = () => {
        const top = (layout.get(key) ?? 0) - feed.scrollTop
        return { top, bottom: top + 20, height: 20 } as DOMRect
      }
    }
  }
  wireRows()
  return { geo, wireRows }
}

/** Newest first, 20 px rows. */
function place(rows: string[]) {
  layout.clear()
  rows.forEach((r, i) => layout.set(r, i * 20))
}

const state = () => screen.getByTestId('state').textContent
const feedEl = () => screen.getByTestId('feed')

afterEach(() => {
  cleanup()
  layout.clear()
})

describe('usePinnedScroll — top edge (#276 newest at top)', () => {
  function mount(rows: string[]) {
    place(rows)
    const view = render(<Feed rows={rows} />)
    const s = stub(feedEl(), { scrollHeight: rows.length * 20, clientHeight: 100 })
    fireEvent.click(screen.getByText('render'))
    return { ...view, ...s }
  }

  it('starts pinned and holds the TOP on every render', () => {
    mount(['r5', 'r4', 'r3', 'r2', 'r1', 'r0', 'q9', 'q8', 'q7', 'q6'])
    const feed = feedEl()
    expect(state()).toBe('pinned')
    feed.scrollTop = 17 // a layout nudge with no scroll event
    fireEvent.click(screen.getByText('render'))
    expect(feed.scrollTop).toBe(0)
  })

  it('drops the pin past the slop from the top, and flips exactly at PIN_SLOP_PX', () => {
    mount(['r5', 'r4', 'r3', 'r2', 'r1', 'r0', 'q9', 'q8', 'q7', 'q6'])
    const feed = feedEl()
    feed.scrollTop = PIN_SLOP_PX
    fireEvent.scroll(feed)
    expect(state()).toBe('pinned')
    feed.scrollTop = PIN_SLOP_PX + 1
    fireEvent.scroll(feed)
    expect(state()).toBe('reviewing')
  })

  it('while reading, rows arriving ABOVE do not move the row being read — even when the bottom is trimmed', () => {
    const first = ['r5', 'r4', 'r3', 'r2', 'r1', 'r0', 'q9', 'q8', 'q7', 'q6']
    const { rerender, geo, wireRows } = mount(first)
    const feed = feedEl()
    // Read from r2 (content 60 px): scrolled down 60.
    feed.scrollTop = 60
    fireEvent.scroll(feed)
    expect(state()).toBe('reviewing')
    // Two new rows on top, two oldest trimmed off the bottom: same height, r2 now at 100 px.
    const next = ['s1', 's0', 'r5', 'r4', 'r3', 'r2', 'r1', 'r0', 'q9', 'q8']
    place(next)
    geo.scrollHeight = next.length * 20
    rerender(<Feed rows={next} />)
    wireRows()
    fireEvent.click(screen.getByText('render'))
    expect(feed.scrollTop).toBe(100) // r2 is still at the top of the view
  })

  it('back within the slop resumes following the top', () => {
    const { rerender, geo, wireRows } = mount(['r5', 'r4', 'r3', 'r2', 'r1', 'r0', 'q9', 'q8'])
    const feed = feedEl()
    feed.scrollTop = 80
    fireEvent.scroll(feed)
    feed.scrollTop = 10
    fireEvent.scroll(feed)
    expect(state()).toBe('pinned')
    const next = ['s0', 'r5', 'r4', 'r3', 'r2', 'r1', 'r0', 'q9', 'q8']
    place(next)
    geo.scrollHeight = next.length * 20
    rerender(<Feed rows={next} />)
    wireRows()
    fireEvent.click(screen.getByText('render'))
    expect(feed.scrollTop).toBe(0)
  })

  it('repin() snaps back to the top after reading', () => {
    mount(['r5', 'r4', 'r3', 'r2', 'r1', 'r0', 'q9', 'q8'])
    const feed = feedEl()
    feed.scrollTop = 80
    fireEvent.scroll(feed)
    fireEvent.click(screen.getByText('repin'))
    expect(state()).toBe('pinned')
    expect(feed.scrollTop).toBe(0)
  })
})
