// @vitest-environment jsdom
//
// The theme chips: Light / Dark / System. The chip ids are the persisted tokens (useTheme's
// ThemeChoice); the words and tooltips come from the catalog.
import { afterEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { ThemeSwitcher } from './ThemeSwitcher'
import type { ThemeChoice } from '../useTheme'
import { EN } from '../i18n'

afterEach(cleanup)

const CHIPS: [ThemeChoice, string][] = [
  ['light', EN['theme.light.label']],
  ['dark', EN['theme.dark.label']],
  ['system', EN['theme.system.label']],
]
const group = () => screen.getByRole('group', { name: EN['theme.aria'] })
const pressed = () =>
  within(group())
    .getAllByRole('button')
    .filter((b) => b.getAttribute('aria-pressed') === 'true')
    .map((b) => b.textContent)

describe('ThemeSwitcher', () => {
  it('offers Light, Dark and System, and presses only the current one, for each of them', () => {
    for (const [id, label] of CHIPS) {
      const { unmount } = render(<ThemeSwitcher theme={id} onChange={() => {}} />)
      expect(within(group()).getAllByRole('button').map((b) => b.textContent)).toEqual(CHIPS.map(([, l]) => l))
      expect(pressed(), `current '${id}'`).toEqual([label])
      unmount()
    }
  })

  it('reports each chip’s own id, System included', () => {
    const changed = vi.fn()
    render(<ThemeSwitcher theme="dark" onChange={changed} />)
    for (const [id, label] of CHIPS) {
      fireEvent.click(within(group()).getByRole('button', { name: label }))
      expect(changed).toHaveBeenLastCalledWith(id)
    }
  })

  it('System says what it follows', () => {
    render(<ThemeSwitcher theme="system" onChange={() => {}} />)
    expect(within(group()).getByRole('button', { name: EN['theme.system.label'] }).getAttribute('title')).toBe(
      EN['theme.system.title'],
    )
  })
})
