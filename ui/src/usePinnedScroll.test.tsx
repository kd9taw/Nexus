// @vitest-environment jsdom
//
// The ONE bottom-pin discipline every append-only feed shares (extracted from
// OperateDecodes — census/prog-scroll: five copies existed, only one checked
// whether the operator had scrolled away before yanking the view). The contract
// pinned here: pinned = follow appended content; scrolled up past the slop =
// the operator is READING and the view is never yanked; back within the slop =
// resume. The re-pin must run on EVERY render, not only on content change —
// that no-dep-array effect is what recovers a keep-alive (display:none) host
// (assessment V17).
//
// jsdom has no layout engine, so scroll geometry is stubbed per element
// (scrollTop writes DO store in jsdom, unclamped — assertions use raw values).
import { describe, it, expect, afterEach } from 'vitest'
import { useState } from 'react'
import { render, screen, fireEvent, cleanup } from '@testing-library/react'
import { usePinnedScroll, PIN_SLOP_PX } from './usePinnedScroll'

function Feed() {
  const { ref, pinned, onScroll, repin } = usePinnedScroll<HTMLDivElement>()
  const [, force] = useState(0)
  return (
    <div>
      <span data-testid="state">{pinned ? 'pinned' : 'reviewing'}</span>
      {/* A content-free re-render — stands in for "a poll landed" / "the
          keep-alive host was shown again and something rendered". */}
      <button onClick={() => force((n) => n + 1)}>render</button>
      <button onClick={repin}>repin</button>
      <div data-testid="feed" ref={ref} onScroll={onScroll} />
    </div>
  )
}

/** Stub scroll geometry on a jsdom element. Mutate the returned object to
 * simulate content growth. */
function stubGeometry(el: HTMLElement, geo: { scrollHeight: number; clientHeight: number }) {
  Object.defineProperty(el, 'scrollHeight', { configurable: true, get: () => geo.scrollHeight })
  Object.defineProperty(el, 'clientHeight', { configurable: true, get: () => geo.clientHeight })
  return geo
}

function mount() {
  render(<Feed />)
  const feed = screen.getByTestId('feed')
  const geo = stubGeometry(feed, { scrollHeight: 1000, clientHeight: 100 })
  // The mount-time snap ran before the stub (scrollHeight 0) — one render
  // brings it to the stubbed bottom.
  fireEvent.click(screen.getByText('render'))
  return { feed, geo }
}

const state = () => screen.getByTestId('state').textContent
const rerender = () => fireEvent.click(screen.getByText('render'))

afterEach(cleanup)

describe('usePinnedScroll', () => {
  it('starts pinned and snaps to the bottom on render', () => {
    const { feed } = mount()
    expect(state()).toBe('pinned')
    expect(feed.scrollTop).toBe(1000)
  })

  it('drops the pin when scrolled up past the slop, and growth never yanks the view', () => {
    const { feed, geo } = mount()
    // Operator scrolls up to read: 200 px from the bottom (> PIN_SLOP_PX).
    feed.scrollTop = 700
    fireEvent.scroll(feed)
    expect(state()).toBe('reviewing')
    // New content lands while reading — the view must NOT move.
    geo.scrollHeight = 1200
    rerender()
    expect(feed.scrollTop).toBe(700)
  })

  it('re-pins when scrolled back within the slop and resumes following', () => {
    const { feed, geo } = mount()
    feed.scrollTop = 700
    fireEvent.scroll(feed)
    // Back to within the slop: distance from bottom = 1000 - 880 - 100 = 20 px.
    feed.scrollTop = 880
    fireEvent.scroll(feed)
    expect(state()).toBe('pinned')
    geo.scrollHeight = 1400
    rerender()
    expect(feed.scrollTop).toBe(1400)
  })

  it('pin state flips exactly at PIN_SLOP_PX from the bottom', () => {
    const { feed } = mount()
    feed.scrollTop = 1000 - 100 - PIN_SLOP_PX // exactly on the slop: still pinned
    fireEvent.scroll(feed)
    expect(state()).toBe('pinned')
    feed.scrollTop = 1000 - 100 - PIN_SLOP_PX - 1 // one px beyond: reading
    fireEvent.scroll(feed)
    expect(state()).toBe('reviewing')
  })

  it('re-pins on a render with NO content change — the keep-alive recovery (V17)', () => {
    const { feed } = mount()
    // A display:none round trip resets the offset with no scroll event (the
    // operator never scrolled). A dep-arrayed effect would skip this render
    // and strand the pane mid-list — this assertion is what protects the
    // deliberately-missing dep array.
    feed.scrollTop = 0
    rerender()
    expect(feed.scrollTop).toBe(1000)
  })

  it('repin() forces a follow again after a scroll-up (Erase/Clear path)', () => {
    const { feed, geo } = mount()
    feed.scrollTop = 700
    fireEvent.scroll(feed)
    expect(state()).toBe('reviewing')
    geo.scrollHeight = 1200
    fireEvent.click(screen.getByText('repin'))
    expect(state()).toBe('pinned')
    expect(feed.scrollTop).toBe(1200)
  })
})
