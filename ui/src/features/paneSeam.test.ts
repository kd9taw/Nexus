// The pure half of the ONE divider (features/paneSeam.ts): what a key does, and the strip clamps
// that moved here from Splitter.tsx unchanged.
import { describe, expect, it } from 'vitest'
import { clampSplitPct, parseSplitPct, seamKey, type SeamKeys } from './paneSeam'

const plain = { shiftKey: false, altKey: false, ctrlKey: false, metaKey: false }
const press = (key: string, mods: Partial<typeof plain> = {}) => ({ key, ...plain, ...mods })

describe('seamKey', () => {
  const y: SeamKeys = { axis: 'y', value: 200, min: 100, max: 400, step: 16, bigStep: 64, grows: 1 }
  const x: SeamKeys = { ...y, axis: 'x' }

  it('the arrows along the axis step the value; the arrow moves the separator the way it points', () => {
    // A strip above its seam grows when the seam moves down.
    expect(seamKey(press('ArrowDown'), y)).toBe(216)
    expect(seamKey(press('ArrowUp'), y)).toBe(184)
    expect(seamKey(press('ArrowRight'), x)).toBe(216)
    expect(seamKey(press('ArrowLeft'), x)).toBe(184)
  })

  it('a pane on the FAR side of its seam grows when the seam moves toward the near side', () => {
    // The Tempo waterfall rail: its seam is on its left edge, so ArrowLeft widens it.
    const right: SeamKeys = { ...x, grows: -1 }
    expect(seamKey(press('ArrowLeft'), right)).toBe(216)
    expect(seamKey(press('ArrowRight'), right)).toBe(184)
  })

  it('Shift makes an arrow a big step', () => {
    expect(seamKey(press('ArrowDown', { shiftKey: true }), y)).toBe(264)
    expect(seamKey(press('ArrowUp', { shiftKey: true }), y)).toBe(136)
  })

  it('Home and End go to the smallest and largest the primary pane may be, whatever the direction', () => {
    expect(seamKey(press('Home'), y)).toBe(100)
    expect(seamKey(press('End'), y)).toBe(400)
    expect(seamKey(press('Home'), { ...x, grows: -1 })).toBe(100)
    expect(seamKey(press('End'), { ...x, grows: -1 })).toBe(400)
  })

  it('Backspace asks for the default back', () => {
    expect(seamKey(press('Backspace'), y)).toBe('reset')
  })

  it('answers nothing for the cross-axis arrows, other keys, or a chord it does not own', () => {
    // Not ours: the event keeps its default and reaches whatever else listens for it.
    expect(seamKey(press('ArrowLeft'), y)).toBeNull()
    expect(seamKey(press('ArrowDown'), x)).toBeNull()
    expect(seamKey(press('Enter'), y)).toBeNull()
    expect(seamKey(press(' '), y)).toBeNull()
    expect(seamKey(press('Delete'), y)).toBeNull()
    for (const mod of ['altKey', 'ctrlKey', 'metaKey'] as const) {
      expect(seamKey(press('ArrowDown', { [mod]: true }), y), mod).toBeNull()
      expect(seamKey(press('Home', { [mod]: true }), y), mod).toBeNull()
      expect(seamKey(press('Backspace', { [mod]: true }), y), mod).toBeNull()
    }
  })

  it('returns the raw target: the caller clamps it against the live box', () => {
    expect(seamKey(press('ArrowDown'), { ...y, value: 395 })).toBe(411)
  })
})

// clampSplitPct: the ONE clamp formula for a split percentage, shared by the drag and
// the mount replay — bounds are [minPx, min(maxPx, 90% of span)], all in CSS px.
describe('clampSplitPct', () => {
  it('passes an in-range percentage through unchanged', () => {
    expect(clampSplitPct(30, 1000, 100, 420)).toBe(30) // 300px ∈ [100, 420]
  })

  it('caps at maxPx (a stored % from a shorter container re-enters range)', () => {
    // 77.8% of 1000 = 778px — the phone-scope worst case from the census: a scope
    // dragged to max at one window size must NOT reopen 60% past its own drag cap.
    expect(clampSplitPct(77.8, 1000, 100, 420)).toBe(42) // 420/1000
  })

  it('floors at minPx', () => {
    expect(clampSplitPct(5, 1000, 100, 420)).toBe(10) // 50px → 100px
  })

  it('caps at 90% of the span when that is tighter than maxPx (same as the drag)', () => {
    expect(clampSplitPct(95, 400, 100, 420)).toBe(90) // min(420, 360)=360 → 90%
  })

  it('returns the input unchanged for a hidden/zero-size container', () => {
    expect(clampSplitPct(88, 0, 100, 420)).toBe(88)
    expect(clampSplitPct(88, -5, 100, 420)).toBe(88)
  })
})

describe('parseSplitPct — a split stored by any earlier build', () => {
  it('reads the percentage Splitter wrote, exactly', () => {
    expect(parseSplitPct('35')).toBe(35)
    expect(parseSplitPct('33.71369628984947')).toBe(33.71369628984947)
  })

  it('reads junk, nothing, and the impossible ends as "never set"', () => {
    for (const raw of [null, '', 'abc', 'NaN', '0', '100', '-4', '140', 'Infinity']) {
      expect(parseSplitPct(raw), String(raw)).toBeNull()
    }
  })
})
