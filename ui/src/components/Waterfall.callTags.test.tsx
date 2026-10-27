// @vitest-environment jsdom
//
// THE FT WATERFALL'S CALLSIGN TAGS, STACKED (#312). The FT cockpit tags each decode of the last
// period over its own audio frequency, and a slot can hold dozens, so the waterfall stacks a crowd
// into rows (`tagLanes`) and counts what fits in none. Without `tagLanes` (RTTY) every tag is drawn
// in the one row, as before. The tag layer's width comes from a ResizeObserver, stubbed here to
// report a fixed width; jsdom lays nothing out.
import { describe, it, expect, beforeEach, afterEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { Waterfall, type WaterfallTag } from './Waterfall'

vi.mock('../api', () => ({
  getSpectrumRow: () => Promise.resolve({ row: [], loHz: 200, hiHz: 4000 }),
}))

const WIDTH = 560 // px: the default 200–3000 Hz view at 5 Hz a pixel

beforeEach(() => {
  localStorage.clear()
  globalThis.ResizeObserver = class {
    constructor(private cb: () => void) {}
    observe() {
      this.cb()
    }
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  vi.spyOn(HTMLElement.prototype, 'clientWidth', 'get').mockReturnValue(WIDTH)
})
afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

const tag = (label: string, hz: number, rank: number, selected = false): WaterfallTag => ({
  key: label,
  hz,
  label,
  title: label,
  opacity: 1,
  dim: false,
  selected,
  rank,
})

function view(tags: WaterfallTag[], props: Partial<React.ComponentProps<typeof Waterfall>> = {}) {
  return render(
    <Waterfall transmitting={false} rxOffsetHz={1500} txOffsetHz={1500} theme="dark" tags={tags} {...props} />,
  )
}

const shown = () => [...document.querySelectorAll<HTMLElement>('.wf-tag')].map((el) => el.textContent)
const top = (label: string) => screen.getByRole('button', { name: label }).style.top

describe('callsign tags on the FT waterfall', () => {
  // Four calls within 40 Hz (8 px) of each other: one row each, and the fourth has none.
  const CROWD = [tag('K1ABC', 1000, 1), tag('W9XYZ', 1010, 0, true), tag('N0CALL', 1020, 2), tag('VE3AAA', 1040, 3)]

  it('stacks a crowd into rows over their own frequencies, and counts the rest', () => {
    view(CROWD, { tagLanes: 3 })
    expect(shown()).toEqual(['K1ABC', 'W9XYZ', 'N0CALL'])
    expect(top('W9XYZ')).toBe('') // rank 0 keeps the top row
    expect(top('K1ABC')).toBe('calc(2px + 1 * (1.4em + 3px))')
    expect(top('N0CALL')).toBe('calc(2px + 2 * (1.4em + 3px))')
    // Each stays centred on its own frequency, whatever its row.
    const left = (label: string) => parseFloat(screen.getByRole('button', { name: label }).style.left)
    expect(left('K1ABC')).toBeCloseTo(((1000 - 200) / 2800) * 100, 6)
    expect(document.querySelector('.wf-tag-more')?.textContent).toBe('+1')
  })

  it('marks the selected station', () => {
    view(CROWD, { tagLanes: 3 })
    expect(screen.getByRole('button', { name: 'W9XYZ' }).className).toBe('wf-tag selected')
    expect(screen.getByRole('button', { name: 'K1ABC' }).className).toBe('wf-tag')
  })

  it('draws every tag in one row without tagLanes, as RTTY has them', () => {
    view(CROWD)
    expect(shown()).toEqual(['K1ABC', 'W9XYZ', 'N0CALL', 'VE3AAA'])
    expect(CROWD.map((t) => top(t.label))).toEqual(['', '', '', ''])
    expect(document.querySelector('.wf-tag-more')).toBeNull()
  })

  it('hands a click on a tag its frequency', () => {
    const onTag = vi.fn()
    view([tag('K1ABC', 1234, 0)], { tagLanes: 3, onTag })
    fireEvent.click(screen.getByRole('button', { name: 'K1ABC' }))
    expect(onTag).toHaveBeenCalledWith(1234)
  })
})
