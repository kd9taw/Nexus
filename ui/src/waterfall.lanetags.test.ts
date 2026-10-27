import { describe, expect, it } from 'vitest'
import { laneTags } from './waterfall'

describe('laneTags — callsign tags stacked over their own frequencies', () => {
  it('keeps labels that clear each other in the first row', () => {
    const { lane, hidden } = laneTags(
      [
        { x: 50, w: 40, rank: 0 },
        { x: 150, w: 40, rank: 1 },
      ],
      400,
      3,
      4,
    )
    expect(lane).toEqual([0, 0])
    expect(hidden).toBe(0)
  })

  it('drops a crowded label to the next row, and the lowest rank keeps the top row', () => {
    const { lane } = laneTags(
      [
        { x: 100, w: 60, rank: 1 },
        { x: 120, w: 60, rank: 0 },
        { x: 140, w: 60, rank: 2 },
      ],
      400,
      3,
      4,
    )
    expect(lane).toEqual([1, 0, 2])
  })

  it('leaves out and counts what fits in no row', () => {
    const { lane, hidden } = laneTags(
      [
        { x: 100, w: 60, rank: 0 },
        { x: 105, w: 60, rank: 1 },
        { x: 110, w: 60, rank: 2 },
      ],
      400,
      2,
      4,
    )
    expect(lane).toEqual([0, 1, null])
    expect(hidden).toBe(1)
  })

  it('judges a label at the edge where it is drawn, inside the picture', () => {
    // Both centred past the left edge: drawn from 0, so they collide there.
    const { lane } = laneTags(
      [
        { x: 0, w: 60, rank: 0 },
        { x: 20, w: 60, rank: 1 },
      ],
      400,
      1,
      4,
    )
    expect(lane).toEqual([0, null])
  })
})
