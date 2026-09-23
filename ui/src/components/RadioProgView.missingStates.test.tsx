// @vitest-environment jsdom
// Issue #241 (swinn): a repeater search that could not read one of the states it planned
// must SAY SO on screen. A short list is otherwise indistinguishable from a quiet area —
// the reporter's two machines were simply absent, with nothing on the panel to suggest the
// list was incomplete.
//
// Both cases are asserted because only the PAIR is evidence: the note must appear when a
// state is missing AND stay away when none is, or a test that always finds it would pass
// against a panel that shows the warning permanently.
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { RepeaterSearchResult } from '../types'

const repeaterSearch = vi.fn()

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  repeaterSearch: (...a: unknown[]) => repeaterSearch(...a),
}))

import { RadioProgView } from './RadioProgView'

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

/** Fetch once and wait for the row to land, so the panel has a result to describe. */
async function fetchWith(missingStates: string[]) {
  repeaterSearch.mockResolvedValue(result(missingStates))
  render(<RadioProgView myGrid="FN31" />)
  fireEvent.click(screen.getByRole('button', { name: /fetch/i }))
  await waitFor(() => expect(screen.getByText('W3ZGD')).toBeTruthy())
}

describe('a state the search never heard from', () => {
  beforeEach(() => {
    cleanup()
    repeaterSearch.mockReset()
    localStorage.clear()
  })

  it('is named on the panel, so a short list cannot read as an empty area', async () => {
    await fetchWith(['MD'])
    // By VALUE: the note names the absent state, not merely "something went wrong".
    const note = await screen.findByText(/RepeaterBook did not answer for/i)
    expect(note.textContent).toContain('MD')
  })

  it('says nothing when every planned state answered', async () => {
    await fetchWith([])
    expect(screen.queryByText(/RepeaterBook did not answer for/i)).toBeNull()
  })
})
