// @vitest-environment jsdom
//
// Ctrl+K (⌘K on a Mac) puts the caret in Settings' search box — the look-and-feel redesign,
// piece 7. The census of every key handler in the app found Ctrl/⌘+K bound nowhere: no cockpit,
// keyer, TX or stop key uses it (the list is in the redesign's report). Held here:
//   · the chord, on each platform, and nothing that merely resembles it;
//   · on a Mac, Ctrl+K stays the text field's own "delete to the end of the line";
//   · no transmit or stop key is ever taken — Escape, Space, the F-keys, Alt+digit, Ctrl+digit;
//   · the listener lives exactly as long as the search box does, and Escape still reaches the
//     window while the box has the caret (CW's and Operate's Esc are window listeners).
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { SettingsSearch, isSearchChord } from './SettingsSearch'
import { EN } from '../i18n'

afterEach(cleanup)

type Chord = { key: string; code?: string; ctrlKey?: boolean; metaKey?: boolean; altKey?: boolean; shiftKey?: boolean }
const ev = (c: Chord) => ({ ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, code: '', ...c })

describe('the chord', () => {
  it('is Ctrl+K on Windows and Linux, ⌘K on a Mac', () => {
    expect(isSearchChord(ev({ key: 'k', code: 'KeyK', ctrlKey: true }), false)).toBe(true)
    expect(isSearchChord(ev({ key: 'k', code: 'KeyK', metaKey: true }), true)).toBe(true)
  })

  it('on a Mac, leaves Ctrl+K to the text field (delete to the end of the line)', () => {
    expect(isSearchChord(ev({ key: 'k', code: 'KeyK', ctrlKey: true }), true)).toBe(false)
  })

  it('off a Mac, leaves the Windows or Super key to the system', () => {
    expect(isSearchChord(ev({ key: 'k', code: 'KeyK', metaKey: true }), false)).toBe(false)
  })

  it('refuses everything that merely resembles it', () => {
    for (const mac of [false, true]) {
      const mod = mac ? { metaKey: true } : { ctrlKey: true }
      expect(isSearchChord(ev({ key: 'k', code: 'KeyK' }), mac), 'plain K').toBe(false)
      expect(isSearchChord(ev({ key: 'K', code: 'KeyK', shiftKey: true, ...mod }), mac), 'Shift').toBe(false)
      expect(isSearchChord(ev({ key: 'k', code: 'KeyK', altKey: true, ...mod }), mac), 'Alt').toBe(false)
      expect(isSearchChord(ev({ key: 'k', code: 'KeyK', ctrlKey: true, metaKey: true }), mac), 'both').toBe(false)
      expect(isSearchChord(ev({ key: 'j', code: 'KeyJ', ...mod }), mac), 'J').toBe(false)
    }
  })

  it('follows the letter on the key, and the key position only where the letter is not Latin', () => {
    // Dvorak: the letter K sits where QWERTY has V — the label wins.
    expect(isSearchChord(ev({ key: 'k', code: 'KeyV', ctrlKey: true }), false)).toBe(true)
    // …and the QWERTY K position types T there, which is not the chord.
    expect(isSearchChord(ev({ key: 't', code: 'KeyK', ctrlKey: true }), false)).toBe(false)
    // A Cyrillic layout types л on that key: no Latin letter to go by, so the position decides.
    expect(isSearchChord(ev({ key: 'л', code: 'KeyK', ctrlKey: true }), false)).toBe(true)
  })

  it('never takes a transmit or stop key, with any modifier', () => {
    const keys: Chord[] = [
      { key: 'Escape', code: 'Escape' },
      { key: ' ', code: 'Space' },
      { key: 'Enter', code: 'Enter' },
      { key: 'PageUp', code: 'PageUp' },
      { key: 'PageDown', code: 'PageDown' },
      ...Array.from({ length: 12 }, (_, i) => ({ key: `F${i + 1}`, code: `F${i + 1}` })),
      ...Array.from({ length: 9 }, (_, i) => ({ key: `${i + 1}`, code: `Digit${i + 1}` })),
    ]
    const mods = [{}, { ctrlKey: true }, { metaKey: true }, { altKey: true }, { shiftKey: true }]
    for (const k of keys)
      for (const m of mods)
        for (const mac of [false, true]) {
          expect(isSearchChord(ev({ ...k, ...m }), mac), `${JSON.stringify({ ...k, ...m })} mac=${mac}`).toBe(false)
        }
  })
})

describe('the search box', () => {
  const box = () => screen.getByRole('combobox', { name: EN['settings.search.label'] }) as HTMLInputElement

  it('takes the caret on Ctrl+K from anywhere in Settings, and cancels the webview’s own use of it', () => {
    render(
      <>
        <input aria-label="another field" />
        <SettingsSearch onPick={() => {}} />
      </>,
    )
    const other = screen.getByRole('textbox', { name: 'another field' })
    other.focus()
    const e = new KeyboardEvent('keydown', { key: 'k', code: 'KeyK', ctrlKey: true, bubbles: true, cancelable: true })
    other.dispatchEvent(e)
    expect(document.activeElement).toBe(box())
    expect(e.defaultPrevented).toBe(true)
  })

  it('shows the chord in the empty box, and names it for assistive tech', () => {
    render(<SettingsSearch onPick={() => {}} />)
    expect(box().getAttribute('aria-keyshortcuts')).toBe('Control+K')
    expect(document.querySelector('.settings-search-kbd')?.textContent).toBe('Ctrl+K')
    fireEvent.change(box(), { target: { value: 'audio' } })
    expect(document.querySelector('.settings-search-kbd'), 'the chord sits over typed text').toBeNull()
  })

  it('stops listening when Settings closes', () => {
    const { unmount } = render(<SettingsSearch onPick={() => {}} />)
    unmount()
    const e = new KeyboardEvent('keydown', { key: 'k', code: 'KeyK', ctrlKey: true, bubbles: true, cancelable: true })
    window.dispatchEvent(e)
    expect(e.defaultPrevented, 'a closed Settings still took Ctrl+K').toBe(false)
  })

  it('lets Escape through to the window while the box has the caret', () => {
    render(<SettingsSearch onPick={() => {}} />)
    const seen = vi.fn()
    window.addEventListener('keydown', seen)
    box().focus()
    fireEvent.keyDown(box(), { key: 'Escape', code: 'Escape' })
    window.removeEventListener('keydown', seen)
    expect(seen).toHaveBeenCalledTimes(1)
    expect((seen.mock.calls[0][0] as KeyboardEvent).defaultPrevented).toBe(false)
  })
})
