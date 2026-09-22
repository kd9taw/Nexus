// @vitest-environment jsdom
//
// #215 — HIGH CONTRAST IS ITS OWN AXIS, AND FIELD MODE IS ONE OF TWO THINGS THAT ASK FOR IT.
//
// The reporter asked for larger fonts AND a higher-contrast theme. The size half already had a
// dedicated control (Settings ▸ Workspace ▸ UI scale — an eleven-step ladder plus a "never
// shrink below this" cap). The contrast half had none: `data-contrast='high'` was reachable
// only through field mode, which in auto scale mode also moves the zoom (`fieldFitScale`). An
// operator who wanted a readable screen at the size they had chosen could not have one.
//
// So the attribute is DERIVED from two inputs — a standing preference and a situation — and
// this file is the truth table. The properties that matter, and each has a test whose
// observable differs when the code is wrong:
//
//   · either input alone lights the tokens (if only `fieldMode` did, this is a second name
//     for field mode and buys nothing);
//   · neither input writes the other's key (the trap: field mode "remembering" a preference
//     the operator never set, or clearing one they did);
//   · the standing preference SURVIVES field mode going off, and field mode going off with no
//     standing preference clears the attribute. Those two are mirrors, and a derivation that
//     dropped either input passes one and fails the other.
//
// The size half is deliberately absent here: `useScale` is keyed on `fieldMode` alone, and
// index-preseed.test.ts holds the first-paint copy of that same asymmetry.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { CONTRAST_STORAGE_KEY, FIELD_STORAGE_KEY, useContrastPrefs } from './useFieldMode'

const attr = () => document.documentElement.getAttribute('data-contrast')

afterEach(() => {
  localStorage.clear()
  document.documentElement.removeAttribute('data-contrast')
  vi.restoreAllMocks()
})

describe('useContrastPrefs: the stored booleans', () => {
  it('both are off on a machine that never chose', () => {
    const { result } = renderHook(() => useContrastPrefs())
    expect(result.current.fieldMode).toBe(false)
    expect(result.current.highContrast).toBe(false)
  })

  it('reads each saved choice from its OWN key', () => {
    localStorage.setItem(CONTRAST_STORAGE_KEY, '1')
    const { result } = renderHook(() => useContrastPrefs())
    expect(result.current.highContrast, 'the standing preference did not load').toBe(true)
    expect(result.current.fieldMode, 'the contrast key was read as field mode').toBe(false)
  })

  it('persists each one without touching the other key', () => {
    const { result } = renderHook(() => useContrastPrefs())
    act(() => result.current.setHighContrast(true))
    expect(localStorage.getItem(CONTRAST_STORAGE_KEY)).toBe('1')
    // The trap this guards: one boolean written through the other's key makes the two
    // controls the same switch again, which is the whole thing being undone.
    expect(localStorage.getItem(FIELD_STORAGE_KEY), 'high contrast wrote the field key').toBe('0')
    act(() => result.current.setFieldMode(true))
    expect(localStorage.getItem(FIELD_STORAGE_KEY)).toBe('1')
    expect(localStorage.getItem(CONTRAST_STORAGE_KEY), 'field mode wrote the contrast key').toBe(
      '1',
    )
    act(() => result.current.setHighContrast(false))
    expect(localStorage.getItem(CONTRAST_STORAGE_KEY)).toBe('0')
    expect(localStorage.getItem(FIELD_STORAGE_KEY), 'clearing contrast cleared field mode').toBe(
      '1',
    )
  })

  it('unreadable storage falls back to off instead of throwing', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    const { result } = renderHook(() => useContrastPrefs())
    expect(result.current.fieldMode).toBe(false)
    expect(result.current.highContrast).toBe(false)
  })
})

describe('useContrastPrefs: data-contrast is derived from both', () => {
  it('neither input: the attribute is absent', () => {
    // The control for every case below. Without it, an implementation that set the attribute
    // unconditionally would pass all three of them.
    renderHook(() => useContrastPrefs())
    expect(attr()).toBeNull()
  })

  it('field mode alone still lights it — the shipped behaviour is unchanged', () => {
    localStorage.setItem(FIELD_STORAGE_KEY, '1')
    renderHook(() => useContrastPrefs())
    expect(attr()).toBe('high')
  })

  it('the standing preference alone lights it — the capability that did not exist', () => {
    // If this one fails, the new switch is decoration: the tokens are exactly what the
    // reporter asked for and nothing else reaches them.
    localStorage.setItem(CONTRAST_STORAGE_KEY, '1')
    renderHook(() => useContrastPrefs())
    expect(attr()).toBe('high')
  })

  it('both inputs: still high, and not doubled into something else', () => {
    localStorage.setItem(FIELD_STORAGE_KEY, '1')
    localStorage.setItem(CONTRAST_STORAGE_KEY, '1')
    renderHook(() => useContrastPrefs())
    expect(attr()).toBe('high')
  })
})

describe('useContrastPrefs: the two inputs do not overwrite each other', () => {
  it('leaving the field keeps a standing high-contrast preference applied', () => {
    localStorage.setItem(CONTRAST_STORAGE_KEY, '1')
    const { result } = renderHook(() => useContrastPrefs())
    act(() => result.current.setFieldMode(true))
    expect(attr()).toBe('high')
    act(() => result.current.setFieldMode(false))
    expect(result.current.highContrast, 'field mode cleared the standing preference').toBe(true)
    expect(attr(), 'leaving the field took the operator’s own contrast with it').toBe('high')
  })

  it('leaving the field with NO standing preference clears it', () => {
    // The mirror of the test above, and the reason that one proves something: a derivation
    // that ignored `fieldMode` and read only the preference passes the first and fails this.
    const { result } = renderHook(() => useContrastPrefs())
    act(() => result.current.setFieldMode(true))
    expect(attr()).toBe('high')
    act(() => result.current.setFieldMode(false))
    expect(attr()).toBeNull()
  })

  it('turning the standing preference off while in the field keeps the tokens', () => {
    // Field mode is still asking for them. A derivation that let the last writer win would
    // strip the attribute here and leave field mode visibly broken.
    const { result } = renderHook(() => useContrastPrefs())
    act(() => result.current.setFieldMode(true))
    act(() => result.current.setHighContrast(true))
    act(() => result.current.setHighContrast(false))
    expect(result.current.fieldMode).toBe(true)
    expect(attr()).toBe('high')
  })
})
