import { describe, it, expect } from 'vitest'
import { tuneTarget } from './waterfall'

const LEFT = 0
const MIDDLE = 1
const RIGHT = 2

// tuneTarget(button, ctrlKey, shiftKey, metaKey)
const t = (button: number, mods: Partial<{ ctrl: boolean; shift: boolean; meta: boolean }> = {}) =>
  tuneTarget(button, mods.ctrl ?? false, mods.shift ?? false, mods.meta ?? false)

describe('waterfall tune gestures', () => {
  // LEFT = RX is identical in stock WSJT-X and JTDX, so it is universal muscle memory and the
  // one gesture that must never move. Nexus once shipped left=TX / right=RX — its own
  // invention — which moved the WRONG marker for an operator arriving from either client.
  it('left click always moves RX', () => {
    expect(t(LEFT)).toBe('rx')
  })

  // Operator ask 2026-07-26: JTDX's right-click-to-set-TX. Additive — stock WSJT-X binds no
  // right-button action at all, so this costs a WSJT-X operator nothing.
  it('right click moves TX (JTDX)', () => {
    expect(t(RIGHT)).toBe('tx')
  })

  it('shift+left also moves TX (WSJT-X), so both conventions work', () => {
    expect(t(LEFT, { shift: true })).toBe('tx')
  })

  it('ctrl+left moves both', () => {
    expect(t(LEFT, { ctrl: true })).toBe('both')
  })

  // Mac QA audit 2026-08-17: on macOS WebKit, Ctrl+left-click IS the OS right-click — it
  // arrives as button 2 WITH ctrlKey set. The old button-2-first order sent the advertised
  // "Ctrl = both" chord down the TX-only arm, silently moving the wrong marker. Ctrl+right
  // is not a real gesture in any mainstream client, so 'both' on every platform costs nothing.
  it('ctrl+right (mac delivery of Ctrl+click) moves both', () => {
    expect(t(RIGHT, { ctrl: true })).toBe('both')
    expect(t(RIGHT, { ctrl: true, shift: true })).toBe('both')
  })

  // Qt maps WSJT-X's Ctrl to ⌘ on macOS, so a mac WSJT-X operator's "both" is Cmd+click.
  // Accepted on every platform — harmless where ⌘ doesn't exist.
  it('cmd+left (metaKey) moves both — mac WSJT-X parity', () => {
    expect(t(LEFT, { meta: true })).toBe('both')
  })

  // A shift-out-of-habit right-click must not silently pick a different marker than aimed;
  // meta+right stays TX too (right-click is the JTDX gesture, ⌘ adds nothing to it).
  it('right click stays TX under shift/meta', () => {
    expect(t(RIGHT, { shift: true })).toBe('tx')
    expect(t(RIGHT, { meta: true })).toBe('tx')
    expect(t(RIGHT, { shift: true, meta: true })).toBe('tx')
  })

  // Ctrl/⌘ beat shift on the left button — one deterministic answer, never an accidental
  // TX-only move when the operator meant both.
  it('ctrl or cmd wins over shift on the left button', () => {
    expect(t(LEFT, { ctrl: true, shift: true })).toBe('both')
    expect(t(LEFT, { meta: true, shift: true })).toBe('both')
  })

  // A mouse's middle/back/forward buttons must never retune the radio. Thumb buttons are easy
  // to catch while dragging across a waterfall, and a silent TX move is an on-air error.
  it('ignores every other button', () => {
    for (const b of [MIDDLE, 3, 4]) {
      expect(t(b)).toBeNull()
      expect(t(b, { ctrl: true, shift: true })).toBeNull()
      expect(t(b, { meta: true })).toBeNull()
    }
  })
})
