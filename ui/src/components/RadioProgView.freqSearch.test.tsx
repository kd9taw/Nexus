// @vitest-environment jsdom
// N46 P2: Program's search box finds the machines ON a typed frequency (147.18, 147.180, 438.5125,
// 438,5125), within 2.5 kHz and whatever the band, digital and on-air filters say, and never a
// neighbouring channel; a callsign or town still filters by text with the filters applied. Each
// machine's links (AllStar, IRLP, DMR ID, a node whose network is not named) and its DMR colour
// code are on its row.
//
// Every case is a PAIR: the thing appears when the data carries it AND stays away when it does
// not, so a panel that always shows it, or never does, cannot pass.
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor } from '@testing-library/react'
import type { RepeaterRecord, RepeaterSearchResult, RepeaterSearchRow } from '../types'
import { t } from '../i18n'

const repeaterSearch = vi.fn()
const listProjects = vi.fn()

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  repeaterSearch: (...a: unknown[]) => repeaterSearch(...a),
  radioprogListProjects: (...a: unknown[]) => listProjects(...a),
}))

import { RadioProgView } from './RadioProgView'

// THE BUDGET (2026-10-09). The slowest case here, "finds every machine on a typed frequency, whatever the…", takes
// 0.30 s and 0.33 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const NOW = Math.floor(Date.now() / 1000)

/** One hearham machine on `mhz` (FM, on the air, unless `over` says otherwise). */
function machine(call: string, mhz: number, id: string, over: Partial<RepeaterRecord> = {}): RepeaterSearchRow {
  return {
    record: {
      source: 'hearham', sourceId: id, callsign: call, outputMhz: mhz, inputMhz: mhz + 0.6, ctcssEncHz: 103.5,
      ctcssDecHz: null, dcs: null, lat: 42.3, lon: -89.0, city: 'Rockford', county: '', state: '', fm: true,
      dmr: false, dstar: false, fusion: false, dmrColorCode: null, bandwidthKhz: null, operational: true,
      openUse: true, updated: null, distanceKm: 10, bearingDeg: 90, ...over,
    },
    channel: {
      id: `hh:${id}`, name: call, rxMhz: mhz, duplex: 'plus', offsetMhz: 0.6, toneMode: 'tone', rtoneHz: 103.5,
      ctoneHz: 103.5, dtcsCode: 23, mode: 'fm', comment: 'Rockford',
      source: { source: 'hearham', sourceId: id, callsign: call },
    },
    sources: [{ source: 'hearham', sourceId: id, channelId: `hh:${id}`, updated: null }],
    disagreements: [],
  }
}

const result = (): RepeaterSearchResult => ({
  lists: [{ source: 'hearham', fetchedUtc: NOW - 86400, stale: false }],
  coverageGap: null,
  missingStates: [],
  rsgbUnavailable: false,
  rsgbBeyond: [],
  rows: [
    machine('W9AAA', 147.18, '1'),
    // On 147.180 too, each hidden by a default filter: digital-only, and off the air.
    machine('K9DMR', 147.1805, '2', { fm: false, dmr: true, dmrColorCode: 3 }),
    machine('W9OFF', 147.18, '3', { operational: false }),
    // The neighbouring channels: the next on a 15 kHz plan, and on a 12.5 kHz one.
    machine('W9NXT', 147.195, '4'),
    machine('W9ALT', 147.1875, '5'),
    machine('GB3UHF', 438.5125, '6'),
    machine('W9LNK', 146.94, '7', {
      dmr: true,
      dmrColorCode: 1,
      links: [
        { network: 'allStar', node: '2462' },
        { network: 'irlp', node: '3570' },
        { network: 'node', node: '7230' },
      ],
    }),
  ],
})

/** The calls the results list shows, in order. */
const calls = () =>
  [...document.querySelectorAll<HTMLElement>('.rp-results .rp-row .rp-call')].map((c) => c.textContent)
const rowOf = (call: string) =>
  [...document.querySelectorAll<HTMLElement>('.rp-results .rp-row')].find(
    (r) => r.querySelector('.rp-call')?.textContent === call,
  )
const search = (text: string) =>
  fireEvent.change(screen.getByRole('searchbox', { name: t('program.filters.search.aria') }), {
    target: { value: text },
  })
const count = () => document.querySelector('.rp-count')?.textContent ?? ''

async function fetched() {
  repeaterSearch.mockResolvedValue(result())
  render(<RadioProgView myGrid="EN52" />)
  fireEvent.click(screen.getByRole('button', { name: /fetch/i }))
  await waitFor(() => expect(rowOf('W9AAA')).toBeTruthy())
}

describe('Program, searched by frequency', () => {
  beforeEach(() => {
    cleanup()
    localStorage.clear()
    repeaterSearch.mockReset()
    listProjects.mockReset()
    listProjects.mockResolvedValue([])
  })

  it('finds every machine on a typed frequency, whatever the filters, and never its neighbours', async () => {
    await fetched()
    // CONTROL: the default filters hide the digital-only and the off-air machine.
    expect(calls()).not.toContain('K9DMR')
    expect(calls()).not.toContain('W9OFF')
    for (const typed of ['147.18', '147.180', ' 147.18 ']) {
      search(typed)
      expect(calls(), typed).toEqual(['W9AAA', 'K9DMR', 'W9OFF'])
      expect(count()).toContain(t('program.count.freq', { shown: 3, freq: '147.18', tol: '2.5' }))
    }
    for (const typed of ['438.5125', '438,5125']) {
      search(typed)
      expect(calls(), typed).toEqual(['GB3UHF'])
    }
  })

  it('still filters by callsign or town, with the filters applied', async () => {
    await fetched()
    search('W9')
    expect(calls()).toEqual(['W9AAA', 'W9NXT', 'W9ALT', 'W9LNK'])
    expect(count()).toContain(t('program.count', { shown: 4, total: 7 }))
    search('Rock')
    expect(calls()).toHaveLength(5)
  })

  it('says when no machine is on the typed frequency, and offers no digital filter it ignores', async () => {
    await fetched()
    search('147.33')
    const empty = document.querySelector('.rp-results .aw-empty')!
    const [before, after] = t('program.results.none.freq', { freq: '147.33', tol: '2.5', radius: '\u0000' }).split('\u0000')
    expect(empty.textContent).toContain(before)
    expect(empty.textContent).toContain(after)
    expect(screen.queryByRole('button', { name: t('program.results.showDigital') })).toBeNull()
    // CONTROL: an empty TEXT search keeps the digital offer.
    search('ZZ9')
    expect(screen.getByRole('button', { name: t('program.results.showDigital') })).toBeTruthy()
  })

  it("shows a machine's links and DMR colour code on its row, and no line for one with neither", async () => {
    await fetched()
    const links = rowOf('W9LNK')!.querySelector('.rp-links')
    expect(links?.textContent).toBe(
      `AllStar 2462 · IRLP 3570 · ${t('program.row.link.node', { node: '7230' })} · CC1`,
    )
    expect(rowOf('W9AAA')!.querySelector('.rp-links')).toBeNull()
    search('147.18')
    expect(rowOf('K9DMR')!.querySelector('.rp-links')?.textContent).toBe('CC3')
  })
})
