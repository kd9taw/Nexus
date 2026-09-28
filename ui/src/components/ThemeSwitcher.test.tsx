// @vitest-environment jsdom
//
// The theme cards: Light / Dark / System, each with a one-line personality. The card ids are the
// persisted tokens (useTheme's ThemeChoice); the words, lines and tooltips come from the catalog.
//
// THE GALLERY (operator picks of 2026-09-27: "All ten", "Keep these names"): the ten built-in
// themes (features/skins.ts) follow the three standard cards, in two groups, "Rig looks" and
// "Modern", each card with its name, its line and a swatch of its page, panel, accent and readout.
// Picking one sets the page to its base and the theme; Dark, Light or System clear it.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { ThemeSwitcher } from './ThemeSwitcher'
import type { ThemeChoice } from '../useTheme'
import { EN } from '../i18n'
import { SKINS, type SkinId } from '../features/skins'

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

describe('ThemeSwitcher — the gallery of built-in themes', () => {
  const E = EN as unknown as Record<string, string>
  const FAMILIES = [
    ['rig', EN['theme.family.rig']],
    ['modern', EN['theme.family.modern']],
  ] as const
  const family = (name: string) => screen.getByRole('group', { name })
  const allCards = () => screen.getAllByRole('button')
  const gallery = (theme: ThemeChoice, skin: SkinId | null, onChange = vi.fn(), onSkinChange = vi.fn()) =>
    render(<ThemeSwitcher theme={theme} skin={skin} onChange={onChange} onSkinChange={onSkinChange} />)

  it('offers the ten after the three standard cards, rig looks then modern, each with its name and line', () => {
    gallery('dark', null)
    expect(cards().map(nameOf)).toEqual(CARDS.map(([, l]) => l))
    for (const [id, name] of FAMILIES) {
      expect(name, `the ${id} group has no name in the catalog`).toBeTruthy()
      const inGroup = within(family(name)).getAllByRole('button')
      const want = SKINS.filter((x) => x.family === id)
      expect(inGroup.map(nameOf), id).toEqual(want.map((x) => E[x.labelKey]))
      inGroup.forEach((card, i) => {
        const line = E[want[i].lineKey]
        expect(line, `${want[i].id} has no line in the catalog`).toBeTruthy()
        const described = (card.getAttribute('aria-describedby') ?? '').split(' ').map((d) => document.getElementById(d)?.textContent)
        expect(described, `${want[i].id}'s line is not its description`).toEqual([line])
      })
    }
    expect(allCards()).toHaveLength(3 + SKINS.length)
  })

  it('paints each theme’s swatch from its table: page, panel, accent, readout', () => {
    gallery('dark', null)
    for (const x of SKINS) {
      const card = screen.getByRole('button', { name: E[x.labelKey] })
      const chips = Array.from(card.querySelectorAll('.theme-card-swatch > span')) as HTMLElement[]
      expect(chips.map((c) => c.style.getPropertyValue('--chip')), x.id).toEqual(
        [x.day['--bg'], x.day['--panel'], x.day['--accent'], x.day['--readout']],
      )
      expect(card.querySelector('.theme-card-swatch')?.getAttribute('aria-hidden'), x.id).toBe('true')
    }
  })

  it('presses only the theme on screen: a built-in one, and then none of the three', () => {
    for (const x of SKINS) {
      const { unmount } = gallery(x.base, x.id)
      expect(allCards().filter((b) => b.getAttribute('aria-pressed') === 'true').map(nameOf), x.id).toEqual([E[x.labelKey]])
      unmount()
    }
    gallery('system', null)
    expect(allCards().filter((b) => b.getAttribute('aria-pressed') === 'true').map(nameOf)).toEqual([EN['theme.system.label']])
  })

  it('a built-in theme is picked with its base; Dark, Light or System clear it', () => {
    const onChange = vi.fn()
    const onSkinChange = vi.fn()
    gallery('dark', 'amber-lcd', onChange, onSkinChange)
    for (const x of SKINS) {
      fireEvent.click(screen.getByRole('button', { name: E[x.labelKey] }))
      expect(onChange, x.id).toHaveBeenLastCalledWith(x.base)
      expect(onSkinChange, x.id).toHaveBeenLastCalledWith(x.id)
    }
    for (const [id, label] of CARDS) {
      fireEvent.click(within(group()).getByRole('button', { name: label }))
      expect(onChange, id).toHaveBeenLastCalledWith(id)
      expect(onSkinChange, id).toHaveBeenLastCalledWith(null)
    }
  })

  it('without the built-in theme wiring it is the three standard cards alone', () => {
    render(<ThemeSwitcher theme="dark" onChange={() => {}} />)
    expect(allCards().map(nameOf)).toEqual(CARDS.map(([, l]) => l))
  })
})
