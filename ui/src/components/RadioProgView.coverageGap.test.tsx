// @vitest-environment jsdom
// #358: the Repeaters screen's coverage-gap note sent the operator to "Settings ▸ Integrations",
// a place Settings does not have. The RepeaterBook API token is in Settings ▸ Logging &
// Connectors ▸ Confirmations, so the note names that place, in every language, from the very
// labels Settings draws, and the place in the note opens it.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { RepeaterSearchResult } from '../types'
import { t } from '../i18n'
import { resolveTarget } from '../settings/registry'
import { EN } from '../i18n/en'
import { DE } from '../i18n/de'
import { ES } from '../i18n/es'
import { FR } from '../i18n/fr'
import { JA } from '../i18n/ja'

const repeaterSearch = vi.fn()

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  repeaterSearch: (...a: unknown[]) => repeaterSearch(...a),
  radioprogListProjects: vi.fn(async () => []),
  radioprogFileNotice: vi.fn(async () => null),
  radioprogSaveProject: vi.fn(async () => undefined),
}))

import { RadioProgView } from './RadioProgView'

const GAP: RepeaterSearchResult = {
  lists: [{ source: 'hearham', fetchedUtc: Math.floor(Date.now() / 1000) - 3600, stale: false }],
  coverageGap: '2m', missingStates: [], rsgbUnavailable: false, rsgbBeyond: [], rows: [],
}

async function showGap(onOpenSettings?: (where: string) => void) {
  repeaterSearch.mockResolvedValue(GAP)
  render(<RadioProgView myGrid="EN52" onOpenSettings={onOpenSettings} />)
  fireEvent.click(screen.getByRole('button', { name: t('program.fetch.label') }))
  await waitFor(() => expect(document.querySelector('.rp-note')).toBeTruthy())
  return document.querySelector<HTMLElement>('.rp-note')!
}

beforeEach(() => {
  cleanup()
  localStorage.clear()
  repeaterSearch.mockReset()
  ;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  }
})
afterEach(cleanup)

describe('the coverage-gap note points at the RepeaterBook token (#358)', () => {
  it('names Settings ▸ Logging & Connectors ▸ Confirmations in every language, never Integrations', () => {
    for (const [lang, cat] of [['en', EN], ['de', DE], ['es', ES], ['fr', FR], ['ja', JA]] as const) {
      const c = cat as Record<string, string>
      const note = c['program.coverageGap']
      expect(note, lang).toContain(`${c['settings.tabs.logging']} ▸ ${c['settings.confirmations.legend']}`)
    }
    expect(EN['program.coverageGap']).not.toMatch(/Settings ▸ Integrations</)
  })

  it('the place in the note opens Settings there', async () => {
    const open = vi.fn()
    const note = await showGap(open)
    const link = note.querySelector('button')
    expect(link, 'the place is a button').toBeTruthy()
    expect(link!.textContent).toBe('Settings ▸ Logging & Connectors ▸ Confirmations')
    fireEvent.click(link!)
    expect(open).toHaveBeenCalledWith('confirmations')
    expect(resolveTarget('confirmations'), 'the target resolves to the Confirmations section').toEqual({
      tab: 'logging',
      section: 'confirmations',
    })
  })

  it('with nowhere to open it, the place is plain bold text', async () => {
    const note = await showGap()
    expect(note.querySelector('button')).toBeNull()
    expect(note.querySelector('strong:last-of-type')!.textContent).toContain('Confirmations')
  })
})
