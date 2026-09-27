// @vitest-environment jsdom
//
// THE COLOUR ROLES' TWO HALVES: the OWNER (usePaletteRoles — App, and each pop-out's own
// document) is the only writer of the `data-<role>` attributes and the `nexus-palette-<role>`
// keys; the READERS (usePaletteKey — the map and the waterfall, which cache token values in a
// canvas) learn that the colours moved without a prop threaded down to them.
//
// Per machine and webview-local, like the theme: a default is NO attribute and NO key, so an
// operator who never opens Settings ▸ Appearance ▸ Colours carries nothing new.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { act, renderHook } from '@testing-library/react'
import { PALETTE_EVENT, usePaletteKey, usePaletteRoles } from './usePaletteRoles'
import { PALETTE_ROLES } from './features/paletteRoles'

const root = () => document.documentElement
const attrs = () => Object.fromEntries(PALETTE_ROLES.map((r) => [r.id, root().getAttribute(r.attr)]))
const keys = () => Object.fromEntries(PALETTE_ROLES.map((r) => [r.id, localStorage.getItem(r.storage)]))
const NONE = Object.fromEntries(PALETTE_ROLES.map((r) => [r.id, null]))

afterEach(() => {
  localStorage.clear()
  for (const r of PALETTE_ROLES) root().removeAttribute(r.attr)
  vi.restoreAllMocks()
})

describe('usePaletteRoles — the owner', () => {
  it('a machine that never chose is on every default, with no attribute and no key', () => {
    const { result } = renderHook(() => usePaletteRoles())
    expect(result.current.palette).toEqual(Object.fromEntries(PALETTE_ROLES.map((r) => [r.id, r.presets[0].id])))
    expect(attrs()).toEqual(NONE)
    expect(keys()).toEqual(NONE)
  })

  it('applies a saved preset on mount', () => {
    localStorage.setItem('nexus-palette-accent', 'violet')
    localStorage.setItem('nexus-palette-ok', 'teal')
    const { result } = renderHook(() => usePaletteRoles())
    expect(result.current.palette.accent).toBe('violet')
    expect(result.current.palette.ok).toBe('teal')
    expect(attrs()).toEqual({ ...NONE, accent: 'violet', ok: 'teal' })
  })

  it('a saved value no preset has lands on the default, not on a dead attribute', () => {
    localStorage.setItem('nexus-palette-readout', 'chartreuse')
    const { result } = renderHook(() => usePaletteRoles())
    expect(result.current.palette.readout).toBe('cyan')
    expect(root().getAttribute('data-readout')).toBeNull()
  })

  it('picking a preset sets the attribute and the key, and tells the readers', () => {
    const heard = vi.fn()
    window.addEventListener(PALETTE_EVENT, heard)
    const { result } = renderHook(() => usePaletteRoles())
    heard.mockClear()
    act(() => result.current.setPreset('readout', 'amber'))
    window.removeEventListener(PALETTE_EVENT, heard)
    expect(result.current.palette.readout).toBe('amber')
    expect(root().getAttribute('data-readout')).toBe('amber')
    expect(localStorage.getItem('nexus-palette-readout')).toBe('amber')
    expect(heard).toHaveBeenCalled()
  })

  it('going back to the default (Reset) removes both the attribute and the key', () => {
    localStorage.setItem('nexus-palette-amber', 'gold')
    const { result } = renderHook(() => usePaletteRoles())
    expect(root().getAttribute('data-amber')).toBe('gold')
    act(() => result.current.setPreset('amber', 'amber'))
    expect(result.current.palette.amber).toBe('amber')
    expect(root().getAttribute('data-amber')).toBeNull()
    expect(localStorage.getItem('nexus-palette-amber')).toBeNull()
  })

  it('one role never moves another', () => {
    const { result } = renderHook(() => usePaletteRoles())
    act(() => result.current.setPreset('cyan', 'blue'))
    act(() => result.current.setPreset('accent', 'blue'))
    expect(attrs()).toEqual({ ...NONE, cyan: 'blue', accent: 'blue' })
    expect(keys()).toEqual({ ...NONE, cyan: 'blue', accent: 'blue' })
  })

  it('unreadable storage falls back to the defaults, and a pick still applies for the session', () => {
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked')
    })
    const { result } = renderHook(() => usePaletteRoles())
    expect(attrs()).toEqual(NONE)
    act(() => result.current.setPreset('ok', 'mint'))
    expect(root().getAttribute('data-ok')).toBe('mint')
  })
})

describe('usePaletteKey — the readers', () => {
  it('changes when the owner changes the palette, and only then', () => {
    const owner = renderHook(() => usePaletteRoles())
    const reader = renderHook(() => usePaletteKey())
    const before = reader.result.current
    owner.rerender()
    expect(reader.result.current, 'a re-render alone moved the key').toBe(before)
    act(() => owner.result.current.setPreset('ok', 'teal'))
    expect(reader.result.current).not.toBe(before)
    const teal = reader.result.current
    act(() => owner.result.current.setPreset('ok', 'green'))
    expect(reader.result.current).toBe(before)
    expect(teal).not.toBe(before)
  })
})
