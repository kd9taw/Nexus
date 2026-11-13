// @vitest-environment jsdom
import { afterEach, describe, expect, it } from 'vitest'
import { cleanup, fireEvent, render, screen } from '@testing-library/react'
import { installCatalog, setLocale } from '../i18n'
import { IT } from '../i18n/it'
import { LOCALE_BETA_NOTE, LOCALE_NATIVE_NAME } from '../i18n/useLocale'
import { LocaleBetaBanner, localeBetaKey } from './LocaleBetaBanner'

afterEach(() => {
  cleanup()
  setLocale('en')
  localStorage.clear()
})

describe('the beta-translation note', () => {
  it('shows in Italian, opens Settings, and stays dismissed once dismissed', () => {
    installCatalog('it', IT)
    setLocale('it')
    let opened = 0
    render(<LocaleBetaBanner onOpenSettings={() => opened++} />)
    const note = screen.getByRole('button', { name: LOCALE_BETA_NOTE.it })
    // The Settings path it names is the Italian catalog's own labels for that path.
    expect(LOCALE_BETA_NOTE.it).toContain(`Impostazioni ▸ ${IT['settings.tabs.appearance']} ▸ ${IT['settings.workspace.language.label']}`)
    fireEvent.click(note)
    expect(opened).toBe(1)
    fireEvent.click(screen.getByRole('button', { name: IT['common.dismiss'] as string }))
    expect(screen.queryByRole('note')).toBeNull()
    expect(localStorage.getItem(localeBetaKey('it'))).toBe('1')
    cleanup()
    render(<LocaleBetaBanner onOpenSettings={() => {}} />)
    expect(screen.queryByRole('note')).toBeNull()
  })

  it('is absent in English (control), and the picker marks Italian as beta', () => {
    setLocale('en')
    render(<LocaleBetaBanner onOpenSettings={() => {}} />)
    expect(screen.queryByRole('note')).toBeNull()
    expect(LOCALE_NATIVE_NAME.it).toBe('Italiano (beta)')
  })
})
