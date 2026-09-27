// @vitest-environment jsdom
//
// The theme cards: Light / Dark / System, each with a one-line personality. The card ids are the
// persisted tokens (useTheme's ThemeChoice); the words, lines and tooltips come from the catalog.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { ThemeSwitcher } from './ThemeSwitcher'
import type { ThemeChoice } from '../useTheme'
import { EN } from '../i18n'

afterEach(cleanup)

const CARDS: [ThemeChoice, string, string][] = [
  ['light', EN['theme.light.label'], EN['theme.light.line']],
  ['dark', EN['theme.dark.label'], EN['theme.dark.line']],
  ['system', EN['theme.system.label'], EN['theme.system.line']],
]
const group = () => screen.getByRole('group', { name: EN['theme.aria'] })
const cards = () => within(group()).getAllByRole('button')
/** A card's accessible name: its theme's name alone, never the line under it. */
const nameOf = (b: HTMLElement) =>
  (b.getAttribute('aria-labelledby') ?? '')
    .split(' ')
    .map((id) => document.getElementById(id)?.textContent ?? '')
    .join(' ')
const pressed = () => cards().filter((b) => b.getAttribute('aria-pressed') === 'true').map(nameOf)

describe('ThemeSwitcher', () => {
  it('offers Light, Dark and System, and presses only the current one, for each of them', () => {
    for (const [id, label] of CARDS) {
      const { unmount } = render(<ThemeSwitcher theme={id} onChange={() => {}} />)
      expect(cards().map(nameOf)).toEqual(CARDS.map(([, l]) => l))
      expect(pressed(), `current '${id}'`).toEqual([label])
      unmount()
    }
  })

  it('reports each card’s own id, System included', () => {
    const changed = vi.fn()
    render(<ThemeSwitcher theme="dark" onChange={changed} />)
    for (const [id, label] of CARDS) {
      fireEvent.click(within(group()).getByRole('button', { name: label }))
      expect(changed).toHaveBeenLastCalledWith(id)
    }
  })

  it('shows each theme’s one-line personality on its card, as the card’s description', () => {
    render(<ThemeSwitcher theme="dark" onChange={() => {}} />)
    for (const [, label, line] of CARDS) {
      expect(line, `${label} has no line in the catalog`).toBeTruthy()
      const card = within(group()).getByRole('button', { name: label })
      expect(card.textContent, `${label}'s card does not show its line`).toContain(line)
      const described = (card.getAttribute('aria-describedby') ?? '')
        .split(' ')
        .map((id) => document.getElementById(id)?.textContent)
      expect(described, `${label}'s line is not its description`).toEqual([line])
    }
  })

  it('System says what it follows', () => {
    render(<ThemeSwitcher theme="system" onChange={() => {}} />)
    expect(within(group()).getByRole('button', { name: EN['theme.system.label'] }).getAttribute('title')).toBe(
      EN['theme.system.title'],
    )
  })
})
