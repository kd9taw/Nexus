import { describe, it, expect } from 'vitest'
import { tuneTarget } from './waterfall'

const LEFT = 0
const MIDDLE = 1
const RIGHT = 2

describe('waterfall tune gestures', () => {
  // LEFT = RX is identical in stock WSJT-X and JTDX, so it is universal muscle memory and the
  // one gesture that must never move. Nexus once shipped left=TX / right=RX — its own
  // invention — which moved the WRONG marker for an operator arriving from either client.
  it('left click always moves RX', () => {
    expect(tuneTarget(LEFT, false, false)).toBe('rx')
  })

  // Operator ask 2026-07-26: JTDX's right-click-to-set-TX. Additive — stock WSJT-X binds no
  // right-button action at all, so this costs a WSJT-X operator nothing.
  it('right click moves TX (JTDX)', () => {
    expect(tuneTarget(RIGHT, false, false)).toBe('tx')
  })

  it('shift+left also moves TX (WSJT-X), so both conventions work', () => {
    expect(tuneTarget(LEFT, false, true)).toBe('tx')
  })

  it('ctrl+left moves both', () => {
    expect(tuneTarget(LEFT, true, false)).toBe('both')
  })

  // Right-click is TX regardless of modifiers: a JTDX operator holding shift out of habit must
  // not silently get a different marker than the one they aimed at.
  it('right click stays TX under any modifier', () => {
    expect(tuneTarget(RIGHT, true, false)).toBe('tx')
    expect(tuneTarget(RIGHT, false, true)).toBe('tx')
    expect(tuneTarget(RIGHT, true, true)).toBe('tx')
  })

  // Ctrl beats shift on the left button — one deterministic answer, never an accidental TX-only
  // move when the operator meant both.
  it('ctrl wins over shift on the left button', () => {
    expect(tuneTarget(LEFT, true, true)).toBe('both')
  })

  // A mouse's middle/back/forward buttons must never retune the radio. Thumb buttons are easy
  // to catch while dragging across a waterfall, and a silent TX move is an on-air error.
  it('ignores every other button', () => {
    for (const b of [MIDDLE, 3, 4]) {
      expect(tuneTarget(b, false, false)).toBeNull()
      expect(tuneTarget(b, true, true)).toBeNull()
    }
  })
})
