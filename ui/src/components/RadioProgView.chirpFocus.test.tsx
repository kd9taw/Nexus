// @vitest-environment jsdom
//
// THE CHIRP HOW-TO GIVES THE KEYBOARD BACK TO "EXPORT FOR CHIRP" when it closes (focusReturn.ts):
// it is opened by code, and Radix returns the keyboard to a Radix trigger only, so it was left on
// the page itself.
import { describe, expect, it, vi, afterEach } from 'vitest'
import { act, cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { RepeaterSearchResult } from '../types'
import { t } from '../i18n'

const repeaterSearch = vi.fn()

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  repeaterSearch: (...a: unknown[]) => repeaterSearch(...a),
}))

import { RadioProgView } from './RadioProgView'

afterEach(() => {
  cleanup()
  localStorage.clear()
})

/** A search result carrying one machine, plus whatever coverage we want to test. */
function result(missingStates: string[]): RepeaterSearchResult {
  return {
    source: 'repeaterbook',
    fetchedUtc: Math.floor(Date.now() / 1000),
    stale: false,
    coverageGap: null,
    missingStates,
    rows: [
      {
        record: {
          source: 'repeaterbook',
          sourceId: '42-1',
          callsign: 'W3ZGD',
          outputMhz: 146.865,
          inputMhz: 146.265,
          ctcssEncHz: null,
          ctcssDecHz: null,
          dcs: null,
          lat: 39.9,
          lon: -76.6,
          city: 'Red Lion',
          county: 'York',
          state: 'Pennsylvania',
          fm: true,
          dmr: false,
          dstar: false,
          fusion: false,
          dmrColorCode: null,
          bandwidthKhz: null,
          operational: true,
          openUse: true,
          distanceKm: 5,
          bearingDeg: 90,
        },
        channel: {
          id: 'w3zgd',
          name: 'W3ZGD',
          rxMhz: 146.865,
          duplex: 'minus',
          offsetMhz: 0.6,
          toneMode: 'tone',
          rtoneHz: 100,
          ctoneHz: 100,
          dtcsCode: 23,
          mode: 'fm',
          comment: 'Red Lion',
          source: { source: 'repeaterbook', sourceId: '42-1', callsign: 'W3ZGD' },
        },
      },
    ],
  }
}

describe('the CHIRP how-to', () => {
  it('gives the keyboard back to Export for CHIRP when it closes', async () => {
    repeaterSearch.mockResolvedValue(result([]))
    render(<RadioProgView myGrid="FN31" />)
    fireEvent.click(screen.getByRole('button', { name: /fetch/i }))
    await waitFor(() => expect(screen.getByText('W3ZGD')).toBeTruthy())
    fireEvent.click(screen.getByRole('button', { name: t('program.row.add.label') }))
    const exportChirp = screen.getByRole('button', { name: t('program.deliver.exportChirp.label') }) as HTMLButtonElement
    await waitFor(() => expect(exportChirp.disabled).toBe(false))
    act(() => exportChirp.focus())
    fireEvent.click(exportChirp)
    await screen.findByRole('dialog')
    fireEvent.keyDown(document.activeElement!, { key: 'Escape' })
    await waitFor(() => expect(screen.queryByRole('dialog')).toBeNull())
    await waitFor(() => expect(document.activeElement).toBe(exportChirp))
  })
})
