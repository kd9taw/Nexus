// @vitest-environment jsdom
// N46 P1: one row per machine, merged from the directories that cover a search. Program names each
// machine's directories and its date, flags a disagreement between them and shows what each
// listed (never resolving it silently), credits RSGB on screen and in the exported file beside the
// other directories, says plainly when the RSGB list could not be read, and finds a channel saved
// from ANY of a machine's rows.
//
// Every case is a PAIR: the thing appears when the data carries it AND stays away when it does
// not, so a panel that always shows it, or never does, cannot pass.
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { RadioProgProject, RepeaterSearchResult, RepeaterSearchRow } from '../types'
import { t } from '../i18n'

const repeaterSearch = vi.fn()
const exportChannels = vi.fn()
const listProjects = vi.fn()

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  repeaterSearch: (...a: unknown[]) => repeaterSearch(...a),
  exportChannels: (...a: unknown[]) => exportChannels(...a),
  radioprogListProjects: (...a: unknown[]) => listProjects(...a),
  saveTextToDownloads: async () => '/downloads/nexus-channels.csv',
}))

import { RadioProgView } from './RadioProgView'

// THE BUDGET (2026-10-09). The slowest case here, "names each machine's directories with its own date, or…", takes
// 0.30 s and 0.21 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const NOW = Math.floor(Date.now() / 1000)
const RSGB_CREDIT = 'Repeater data: RSGB ETCC (ukrepeater.net)'
const RB_CREDIT = 'Data courtesy of RepeaterBook.com'
const HEARHAM_CREDIT = 'Repeater data from hearham.com'

/** One machine as the station sends it, FM on `mhz`; `from` is its directory rows, top first. */
function machine(
  call: string,
  mhz: number,
  from: RepeaterSearchRow['sources'],
  disagreements: RepeaterSearchRow['disagreements'] = [],
): RepeaterSearchRow {
  const top = from[0]!
  return {
    record: {
      source: top.source, sourceId: top.sourceId, callsign: call, outputMhz: mhz, inputMhz: mhz + 7.6,
      ctcssEncHz: 88.5, ctcssDecHz: null, dcs: null, lat: 53.6, lon: -1.4, city: 'WAKEFIELD', county: '',
      state: '', fm: true, dmr: false, dstar: false, fusion: false, dmrColorCode: null, bandwidthKhz: 12.5,
      operational: true, openUse: true, updated: top.updated ?? null, distanceKm: 20, bearingDeg: 80,
    },
    channel: {
      id: top.channelId, name: call, rxMhz: mhz, duplex: 'plus', offsetMhz: 7.6, toneMode: 'tone', rtoneHz: 88.5,
      ctoneHz: 88.5, dtcsCode: 23, mode: 'nfm', comment: 'WAKEFIELD',
      source: { source: top.source, sourceId: top.sourceId, callsign: call },
    },
    sources: from,
    disagreements,
  }
}

/** GB3BW as the merge gives it: RSGB's row on top, hearham's two behind, tone and input disputed. */
const bw = () =>
  machine(
    'GB3BW',
    430.8125,
    [
      { source: 'rsgb', sourceId: '4808', channelId: 'rsgb:4808', updated: null },
      { source: 'hearham', sourceId: '12141', channelId: 'hh:12141', updated: null },
      { source: 'hearham', sourceId: '12268', channelId: 'hh:12268', updated: null },
    ],
    [
      { field: 'tone', said: [{ source: 'rsgb', value: '88.5' }, { source: 'hearham', value: '82.5' }] },
      {
        field: 'input',
        said: [
          { source: 'rsgb', value: '438.4125' },
          { source: 'hearham', value: '438.4125' },
          { source: 'hearham', value: '430.8125' },
        ],
      },
    ],
  )
/** A RepeaterBook machine with its own date, its directories agreeing. */
const zgd = () =>
  machine('W3ZGD', 146.865, [{ source: 'repeaterbook', sourceId: '42-1', channelId: 'rb:42-1', updated: '2026-05-14' }])
/** A hearham machine and nothing else (on 70 cm, which the default band chips show). */
const itw = () => machine('MB7ITW', 433.4, [{ source: 'hearham', sourceId: '671', channelId: 'hh:671', updated: null }])

function result(over: Partial<RepeaterSearchResult> = {}): RepeaterSearchResult {
  return {
    lists: [
      { source: 'rsgb', fetchedUtc: NOW - 2 * 86400, stale: false },
      { source: 'repeaterbook', fetchedUtc: NOW - 86400, stale: false },
      { source: 'hearham', fetchedUtc: NOW - 3 * 86400, stale: false },
    ],
    coverageGap: null,
    missingStates: [],
    rsgbUnavailable: false,
    rsgbBeyond: [],
    rows: [bw(), zgd()],
    ...over,
  }
}
const hearhamAlone = (over: Partial<RepeaterSearchResult> = {}) =>
  result({ lists: [{ source: 'hearham', fetchedUtc: NOW - 3 * 86400, stale: false }], rows: [itw()], ...over })

/** The results row for a callsign (the channel list beside it is not searched). */
const rowOf = (call: string) =>
  [...document.querySelectorAll<HTMLElement>('.rp-results .rp-row')].find(
    (r) => r.querySelector('.rp-call')?.textContent === call,
  )

async function fetchWith(res: RepeaterSearchResult, first: string) {
  repeaterSearch.mockResolvedValue(res)
  render(<RadioProgView myGrid="IO83" />)
  fireEvent.click(screen.getByRole('button', { name: /fetch/i }))
  await waitFor(() => expect(rowOf(first)).toBeTruthy())
}

describe('Program, merged from several directories', () => {
  beforeEach(() => {
    cleanup()
    localStorage.clear()
    repeaterSearch.mockReset()
    exportChannels.mockReset()
    listProjects.mockReset()
    listProjects.mockResolvedValue([])
  })

  it("names each machine's directories with its own date, or no date and its list's age", async () => {
    await fetchWith(result(), 'GB3BW')
    const age = t('program.age.days', { days: 2 })
    expect(rowOf('GB3BW')!.querySelector('.rp-src')!.textContent).toBe(
      t('program.row.source.noDate', { sources: 'RSGB + hearham', age }),
    )
    expect(rowOf('W3ZGD')!.querySelector('.rp-src')!.textContent).toBe(
      t('program.row.source.updated', { sources: 'RepeaterBook', date: '2026-05-14' }),
    )
  })

  it('flags a disagreement and shows what each directory listed, the programmed value first', async () => {
    await fetchWith(result(), 'GB3BW')
    const row = rowOf('GB3BW')!
    expect(within(row).getByText(t('program.row.differ.label'))).toBeTruthy()
    expect(within(row).getByText(t('program.row.differ.tone', { used: 'RSGB 88.5', others: 'hearham 82.5' }))).toBeTruthy()
    // hearham's agreeing 438.4125 is not "also listed": only the value that differs is.
    expect(
      within(row).getByText(t('program.row.differ.input', { used: 'RSGB 438.4125', others: 'hearham 430.8125' })),
    ).toBeTruthy()
    // CONTROL: the machine its directories agree about carries neither.
    const agreed = rowOf('W3ZGD')!
    expect(within(agreed).queryByText(t('program.row.differ.label'))).toBeNull()
    expect(agreed.querySelector('.rp-differ')).toBeNull()
  })

  it('credits every directory the list came from, on screen and as a line each in the exported file', async () => {
    exportChannels.mockResolvedValue('csv text')
    await fetchWith(result(), 'GB3BW')
    const rsgbLink = screen.getByRole('link', { name: RSGB_CREDIT })
    expect(rsgbLink.getAttribute('href')).toBe('https://ukrepeater.net')
    expect(screen.getByRole('link', { name: HEARHAM_CREDIT })).toBeTruthy()
    fireEvent.click(within(rowOf('GB3BW')!).getByRole('button', { name: t('program.row.add.label') }))
    fireEvent.click(screen.getByRole('button', { name: t('program.deliver.exportCsv.label') }))
    await waitFor(() => expect(exportChannels).toHaveBeenCalled())
    expect(String(exportChannels.mock.calls[0]![3]).split('\n')).toEqual([RSGB_CREDIT, RB_CREDIT, HEARHAM_CREDIT])
  })

  it("CONTROL: a list that is hearham's alone credits hearham alone, on screen and in the file", async () => {
    exportChannels.mockResolvedValue('csv text')
    await fetchWith(hearhamAlone(), 'MB7ITW')
    expect(screen.queryByRole('link', { name: RSGB_CREDIT })).toBeNull()
    fireEvent.click(within(rowOf('MB7ITW')!).getByRole('button', { name: t('program.row.add.label') }))
    fireEvent.click(screen.getByRole('button', { name: t('program.deliver.exportCsv.label') }))
    await waitFor(() => expect(exportChannels).toHaveBeenCalled())
    expect(exportChannels.mock.calls[0]![3]).toBe(HEARHAM_CREDIT)
  })

  it('says plainly when the RSGB list could not be read, and only then', async () => {
    await fetchWith(hearhamAlone({ rsgbUnavailable: true }), 'MB7ITW')
    const note = await screen.findByText(/repeater list could not be read/)
    expect(note.textContent).toContain('RSGB')
    cleanup()
    await fetchWith(hearhamAlone(), 'MB7ITW')
    expect(screen.queryByText(/repeater list could not be read/)).toBeNull()
  })

  // 2026-10-02: hearham is the list under the others. When it could not be read while another
  // directory answered, the panel says so; with nothing to show it says so in the list's place,
  // never "No FM repeaters within 50 mi.", since the area is then not known to be empty.
  const rbAlone = (over: Partial<RepeaterSearchResult> = {}) =>
    result({ lists: [{ source: 'repeaterbook', fetchedUtc: NOW - 86400, stale: false }], rows: [zgd()], ...over })
  const HEARHAM_UNREAD = /hearham repeater list could not be read/

  it("says when hearham's list could not be read behind another directory's, and only then", async () => {
    await fetchWith(rbAlone({ hearhamUnavailable: true }), 'W3ZGD')
    expect((await screen.findByText(HEARHAM_UNREAD)).closest('.rp-note')).toBeTruthy()
    cleanup()
    await fetchWith(rbAlone(), 'W3ZGD')
    expect(screen.queryByText(HEARHAM_UNREAD)).toBeNull()
  })

  it("with nothing to show and hearham's list missing, never says there are no repeaters", async () => {
    const emptyWords = async (res: RepeaterSearchResult) => {
      cleanup()
      repeaterSearch.mockResolvedValue(res)
      render(<RadioProgView myGrid="EN52" />)
      fireEvent.click(screen.getByRole('button', { name: /fetch/i }))
      return waitFor(() => {
        const empty = document.querySelector('.rp-results .aw-empty')
        expect(empty).toBeTruthy()
        return empty!.textContent ?? ''
      })
    }
    const unread = await emptyWords(rbAlone({ rows: [], hearhamUnavailable: true }))
    expect(unread).toMatch(HEARHAM_UNREAD)
    expect(unread).not.toMatch(/No (FM )?repeaters? (with)?in/)
    // CONTROL: the same empty list with hearham's read is the area's own answer, and says that.
    const read = await emptyWords(rbAlone({ rows: [] }))
    expect(read).toMatch(/No FM repeaters within/)
    expect(read).not.toMatch(HEARHAM_UNREAD)
  })

  it('names the locator squares RSGB was not asked about, and only when there are some', async () => {
    await fetchWith(result({ rsgbBeyond: ['IO70', 'IO80'] }), 'GB3BW')
    const note = await screen.findByText(/is asked about the locator squares nearest you/)
    expect(note.textContent).toContain('IO70, IO80')
    cleanup()
    await fetchWith(result(), 'GB3BW')
    expect(screen.queryByText(/is asked about the locator squares nearest you/)).toBeNull()
  })

  it('finds a channel saved from any of a machine\'s rows, and removes that one', async () => {
    const saved = { ...bw().channel, id: 'hh:12141' }
    listProjects.mockResolvedValue([
      { id: 'working', name: 'My channels', createdUtc: 0, updatedUtc: 0, radiusKm: 40, channels: [saved] },
    ] as unknown as RadioProgProject[])
    await fetchWith(result(), 'GB3BW')
    const button = (call: string, label: string) =>
      within(rowOf(call)!).queryByRole('button', { name: t(label as 'program.row.add.label') })
    await waitFor(() => expect(button('GB3BW', 'program.row.added.label')).toBeTruthy())
    // CONTROL: the machine nothing was saved from.
    expect(button('W3ZGD', 'program.row.add.label')).toBeTruthy()
    // Pressing it takes the saved hearham channel out; it does not add an RSGB twin beside it.
    fireEvent.click(button('GB3BW', 'program.row.added.label')!)
    await waitFor(() => expect(button('GB3BW', 'program.row.add.label')).toBeTruthy())
  })
})
