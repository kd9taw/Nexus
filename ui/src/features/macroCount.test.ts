// HOW MANY MACRO KEYS A KEYBOARD DOCK SHOWS — 8, 12 or 16 (`macroSets.ts`, the count model shared by
// CW, RTTY and PSK). The keys F1–F12 are sent by their F-key; keys 13–16 only by a click, so they
// carry no key name and no keyboard shortcut. The count is only how many keys SHOW: a set's saved
// keys past it are kept, hidden, and come back when the count does.
import { describe, expect, it } from 'vitest'
import {
  MACRO_COUNTS,
  dockMacroKey,
  macroCount,
  macroEditChordKey,
  macroKeyCaption,
  macroKeys,
  resolveMacroSet,
  withMacroEntry,
  type BuiltinMacro,
  type MacroSetId,
} from './macroSets'
import { resolvePskSet } from './pskMacros'
import { resolveRttySet } from './rttyMacros'
import { cwVisibleMacros } from './cwMacros'
import type { KeyboardMacroProfile } from '../types'

const echo = (key: string) => key
const press = (key: string, mods: Partial<Record<'shiftKey' | 'ctrlKey' | 'altKey' | 'metaKey', boolean>> = {}) => ({
  key,
  shiftKey: false,
  ctrlKey: false,
  altKey: false,
  metaKey: false,
  ...mods,
})

describe('the count a setting holds', () => {
  it('is 8, 12 or 16, and 8 for anything else — an old file has none', () => {
    expect(MACRO_COUNTS).toEqual([8, 12, 16])
    expect(macroCount(undefined)).toBe(8)
    expect(macroCount(12)).toBe(12)
    expect(macroCount(16)).toBe(16)
    for (const bad of [0, 9, 10, 13, 17, '16', null, NaN]) expect(macroCount(bad)).toBe(8)
  })

  it('names the keys it shows, in keyboard order', () => {
    expect(macroKeys(8)).toEqual(['F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8'])
    expect(macroKeys(12)).toEqual([...macroKeys(8), 'F9', 'F10', 'F11', 'F12'])
    expect(macroKeys(16)).toEqual([...macroKeys(12), 'F13', 'F14', 'F15', 'F16'])
  })

  it('captions F1–F12 by their key, and 13–16 with none: they have no key to press', () => {
    expect(macroKeys(16).map(macroKeyCaption)).toEqual([
      'F1', 'F2', 'F3', 'F4', 'F5', 'F6', 'F7', 'F8', 'F9', 'F10', 'F11', 'F12', '', '', '', '',
    ])
  })
})

describe('the keys a press sends', () => {
  it('is the bare F-key of a key the dock shows: F9–F12 at 12 and 16, never at 8', () => {
    for (const k of ['F9', 'F10', 'F11', 'F12']) {
      expect(dockMacroKey(press(k), macroKeys(8)), `${k} at 8`).toBeNull()
      expect(dockMacroKey(press(k), macroKeys(12)), `${k} at 12`).toBe(k)
      expect(dockMacroKey(press(k), macroKeys(16)), `${k} at 16`).toBe(k)
    }
    expect(dockMacroKey(press('F1'), macroKeys(8))).toBe('F1')
    expect(dockMacroKey(press('F8', { shiftKey: true }), macroKeys(8)), 'Shift+F8 sent before and still does').toBe('F8')
  })

  it('is never keys 13–16, even on a keyboard that has an F13', () => {
    for (const k of ['F13', 'F14', 'F15', 'F16']) expect(dockMacroKey(press(k), macroKeys(16))).toBeNull()
  })

  it("leaves the system's chords alone: Alt, Ctrl or Cmd held, and Shift+F10 (the context menu)", () => {
    for (const mod of ['altKey', 'ctrlKey', 'metaKey'] as const)
      expect(dockMacroKey(press('F11', { [mod]: true }), macroKeys(12))).toBeNull()
    expect(dockMacroKey(press('F10', { shiftKey: true }), macroKeys(12))).toBeNull()
    expect(dockMacroKey(press('F9', { shiftKey: true }), macroKeys(12)), 'control: Shift with another key').toBe('F9')
  })

  it('opens the editor of F9–F12 on Ctrl+Shift where they show, and of 13–16 never', () => {
    const chord = (k: string) => press(k, { ctrlKey: true, shiftKey: true })
    expect(macroEditChordKey(chord('F12'), macroKeys(12))).toBe('F12')
    expect(macroEditChordKey(chord('F12'), macroKeys(8))).toBeNull()
    expect(macroEditChordKey(chord('F13'), macroKeys(16))).toBeNull()
    expect(macroEditChordKey(chord('F8')), 'control: F1–F8 as before').toBe('F8')
  })
})

describe("a keyboard set at each count (RTTY's and PSK's)", () => {
  const BUILTINS: Record<MacroSetId, BuiltinMacro[]> = {
    everyday: [{ key: 'F1', label: 'CQ', text: 'CQ' }],
    contest: [],
  }
  const saved = (keys: string[]): KeyboardMacroProfile[] => [
    { name: 'everyday', macros: keys.map((key) => ({ key, label: `L${key}`, text: `T${key}` })) },
  ]

  it('shows the count of keys, the ones past the built-ins empty', () => {
    for (const n of [8, 12, 16] as const) {
      const slots = resolveMacroSet([], 'everyday', BUILTINS, echo, n)
      expect(slots.map((s) => s.key)).toEqual(macroKeys(n))
      expect(slots[0]).toMatchObject({ label: 'CQ', text: 'CQ', custom: false })
      expect(slots.slice(1).every((s) => s.label === '' && s.text === '' && !s.custom)).toBe(true)
    }
  })

  it('shows the eight it always showed when no count is given (8, the default)', () => {
    expect(resolveRttySet([], 'contest', echo).map((s) => s.key)).toEqual(macroKeys(8))
    expect(resolvePskSet([], 'everyday', echo).map((s) => s.key)).toEqual(macroKeys(8))
    expect(resolveRttySet([], 'contest', echo, 16).map((s) => s.key)).toEqual(macroKeys(16))
    expect(resolvePskSet([], 'everyday', echo, 12).map((s) => s.key)).toEqual(macroKeys(12))
  })

  it('hides the saved keys past the count, and shows them again when the count comes back', () => {
    const all = saved(macroKeys(16) as unknown as string[])
    expect(resolveMacroSet(all, 'everyday', BUILTINS, echo, 8).map((s) => s.label)).toEqual(
      macroKeys(8).map((k) => `L${k}`),
    )
    expect(resolveMacroSet(all, 'everyday', BUILTINS, echo, 16).map((s) => s.label)).toEqual(
      macroKeys(16).map((k) => `L${k}`),
    )
  })

  it('saves a key past F8 in keyboard order, and an edit at 8 keeps the hidden keys', () => {
    let p = withMacroEntry([], 'everyday', 'F16', { label: 'a', text: 'b' })
    p = withMacroEntry(p, 'everyday', 'F10', { label: 'c', text: 'd' })
    p = withMacroEntry(p, 'everyday', 'F2', { label: 'e', text: 'f' })
    expect(p[0].macros.map((m) => m.key)).toEqual(['F2', 'F10', 'F16'])
    p = withMacroEntry(p, 'everyday', 'F2', null)
    expect(p[0].macros.map((m) => m.key), 'the hidden keys survive an edit of a shown one').toEqual(['F10', 'F16'])
  })
})

describe("a CW profile at each count", () => {
  const eight = macroKeys(8).map((key) => ({ key, label: key, text: `T${key}` }))

  it('is the list the dock always showed at 8, and the keys past it hidden', () => {
    expect(cwVisibleMacros(eight, 8)).toEqual(eight)
    const sixteen = [...eight, ...macroKeys(16).slice(8).map((key) => ({ key, label: key, text: `T${key}` }))]
    expect(cwVisibleMacros(sixteen, 8)).toEqual(eight)
    expect(cwVisibleMacros(sixteen, 16)).toEqual(sixteen)
  })

  it('adds the keys past F8 in keyboard order, empty where the profile has none', () => {
    const some = [...eight, { key: 'F11', label: 'X', text: 'Y' }, { key: 'F9', label: 'P', text: 'Q' }]
    expect(cwVisibleMacros(some, 12)).toEqual([
      ...eight,
      { key: 'F9', label: 'P', text: 'Q' },
      { key: 'F10', label: '', text: '' },
      { key: 'F11', label: 'X', text: 'Y' },
      { key: 'F12', label: '', text: '' },
    ])
  })
})
