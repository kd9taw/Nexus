// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, render, screen } from '@testing-library/react'
import { installCatalog, setLocale } from '../i18n'
import { IT } from '../i18n/it'
import { LOCALE_BETA_NOTE, LOCALE_NATIVE_NAME } from '../i18n/useLocale'
import { LocaleBetaNote } from './LocaleBetaNote'

afterEach(() => {
  cleanup()
  setLocale('en')
})

describe('the beta-translation note', () => {
  it('reads in Italian, names the real Settings path, and links the issues page', () => {
    installCatalog('it', IT)
    setLocale('it')
    render(<LocaleBetaNote className="settings-hint" />)
    const note = screen.getByRole('note')
    expect(note.textContent).toContain(`Impostazioni ▸ ${IT['settings.tabs.appearance']} ▸ ${IT['settings.workspace.language.label']}`)
    const link = screen.getByRole('link', { name: LOCALE_BETA_NOTE.it.link })
    expect(link.getAttribute('href')).toBe('https://github.com/kd9taw/Nexus/issues')
  })

  it('renders nothing in English (control), and the picker marks Italian as beta', () => {
    setLocale('en')
    const { container } = render(<LocaleBetaNote className="settings-hint" />)
    expect(container.innerHTML).toBe('')
    expect(LOCALE_NATIVE_NAME.it).toBe('Italiano (beta)')
  })
})
