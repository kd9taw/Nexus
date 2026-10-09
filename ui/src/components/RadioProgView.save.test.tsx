// @vitest-environment jsdom
// Save to Memories (the operator's pick, 2026-10-02: "Save buttons + all fields"): a "Save to Memories" button on every
// FM row and "Save all shown" beside the count. Each memory carries the frequency, offset, tone or DCS, narrow, the
// callsign, and the town and links (AllStar, IRLP, DMR ID, colour code) in its notes. A badge marks a machine already
// saved, and nothing is saved twice: "the same machine" is the merge's own rule (`sameMachine`), so a machine saved
// under another directory's reading of it is found, and another machine on the same output and tone is not.
//
// Every case is a PAIR: what saving does beside what it must leave alone.
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { cleanup, fireEvent, render, screen, waitFor, within } from '@testing-library/react'
import type { RepeaterRecord, RepeaterSearchResult, RepeaterSearchRow } from '../types'
import { t } from '../i18n'
import { emptyBank, memoriesStore, type Memory } from '../features/memories'

const repeaterSearch = vi.fn()
const toasts = vi.hoisted(() => [] as string[])

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  repeaterSearch: (...a: unknown[]) => repeaterSearch(...a),
  radioprogListProjects: vi.fn(async () => []),
  radioprogFileNotice: vi.fn(async () => null),
  radioprogSaveProject: vi.fn(async () => undefined),
}))
vi.mock('../toast', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../toast')>()),
  pushToast: (m: string) => toasts.push(m),
}))

import { RadioProgView } from './RadioProgView'

// THE BUDGET (2026-10-09). The slowest case here, "saves the machine with every field the operator listed…", takes
// 0.26 s and 0.32 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

/** One machine. Out `mhz`, in 600 kHz below, a CTCSS tone, at Janesville unless told otherwise. */
function machine(call: string, mhz: number, over: Partial<RepeaterRecord> = {}, chan: Partial<RepeaterSearchRow['channel']> = {}): RepeaterSearchRow {
  const record: RepeaterRecord = {
    source: 'repeaterbook', sourceId: `rb-${call}`, callsign: call, outputMhz: mhz, inputMhz: mhz - 0.6,
    ctcssEncHz: 103.5, ctcssDecHz: null, dcs: null, lat: 42.68, lon: -89.02, city: 'Janesville', county: '', state: 'WI',
    fm: true, dmr: false, dstar: false, fusion: false, dmrColorCode: null, bandwidthKhz: null,
    operational: true, openUse: true, updated: null, distanceKm: 10, bearingDeg: 90, ...over,
  }
  return {
    record,
    channel: {
      id: `rb:${call}`, name: call, rxMhz: mhz, duplex: 'minus', offsetMhz: 0.6, toneMode: 'tone', rtoneHz: 103.5,
      ctoneHz: 103.5, dtcsCode: 23, mode: 'fm', comment: record.city,
      source: { source: record.source, sourceId: record.sourceId, callsign: call }, ...chan,
    },
    sources: [{ source: record.source, sourceId: record.sourceId, channelId: `rb:${call}`, updated: null }],
    disagreements: [],
  }
}
/** Narrow, linked, with a colour code: every field the operator listed. */
const LINKED = machine('W9ABC', 146.94, { bandwidthKhz: 12.5, links: [{ network: 'allStar', node: '2462' }], dmrColorCode: 1 }, {
  mode: 'nfm', links: ['AllStar 2462', 'IRLP 3570', 'DMR ID 314158'], dmrColorCode: 1,
})
/** A DCS machine. */
const DCS = machine('K9DCS', 442.725, { ctcssEncHz: null, dcs: 23, city: 'Beloit' }, { duplex: 'plus', offsetMhz: 5, toneMode: 'dtcs', rtoneHz: 88.5, ctoneHz: 88.5 })
/** Digital only: listed, never programmed. */
const DMR_ONLY = machine('W9DMR', 443.1, { fm: false, dmr: true }, { mode: 'dmr' })

const result = (rows: RepeaterSearchRow[]): RepeaterSearchResult => ({
  lists: [{ source: 'repeaterbook', fetchedUtc: Math.floor(Date.now() / 1000) - 3600, stale: false }],
  coverageGap: null, missingStates: [], rsgbUnavailable: false, rsgbBeyond: [], rows,
})

const rowOf = (call: string) =>
  [...document.querySelectorAll<HTMLElement>('.rp-results .rp-row')].find((r) => r.querySelector('.rp-call')?.textContent === call)!
const saveIn = (call: string) => within(rowOf(call)).queryByRole('button', { name: t('program.row.save.label') })
const badgeIn = (call: string) => rowOf(call).querySelector<HTMLElement>('.rp-saved-badge')
const memories = (): Memory[] => memoriesStore.get().memories
const saveAll = () => screen.queryByRole('button', { name: t('program.saveAll.label') })

async function fetched(rows: RepeaterSearchRow[], shown = rows.length) {
  repeaterSearch.mockResolvedValue(result(rows))
  render(<RadioProgView myGrid="EN52" />)
  fireEvent.click(screen.getByRole('button', { name: t('program.fetch.label') }))
  await waitFor(() => expect(document.querySelectorAll('.rp-results .rp-row').length).toBe(shown))
}

describe('Save to Memories', () => {
  beforeEach(() => {
    cleanup()
    localStorage.clear()
    memoriesStore.set(emptyBank())
    toasts.length = 0
    repeaterSearch.mockReset()
  })
  afterEach(cleanup)

  it('saves the machine with every field the operator listed, and its row shows the badge instead', async () => {
    await fetched([LINKED, DCS])
    // CONTROL: nothing saved yet, so every FM row offers the button and none shows the badge.
    expect([saveIn('W9ABC'), saveIn('K9DCS')].every(Boolean)).toBe(true)
    expect(badgeIn('W9ABC')).toBeNull()

    fireEvent.click(saveIn('W9ABC')!)
    expect(memories()).toHaveLength(1)
    expect(memories()[0]).toMatchObject({
      name: 'W9ABC 94', rxMhz: 146.94, mode: 'NFM', kind: 'repeater', offsetDir: 'minus', offsetMhz: 0.6,
      toneMode: 'tone', ctcssEncHz: 103.5, callsign: 'W9ABC',
      notes: 'Janesville; AllStar 2462; IRLP 3570; DMR ID 314158; CC1', lat: 42.68, lon: -89.02,
      source: 'program', favorite: false,
    })
    await waitFor(() => expect(badgeIn('W9ABC')).not.toBeNull())
    expect(badgeIn('W9ABC')!.textContent).toBe(t('program.row.saved.label'))
    expect(badgeIn('W9ABC')!.title).toBe(t('program.row.saved.title', { name: 'W9ABC 94' }))
    expect(saveIn('W9ABC')).toBeNull()
    expect(toasts).toContain(t('program.save.done', { name: 'W9ABC 94' }))
    // The other row is untouched: still unsaved, still offering the button.
    expect(saveIn('K9DCS')).not.toBeNull()
    expect(badgeIn('K9DCS')).toBeNull()

    // A DCS machine carries its code, not a tone.
    fireEvent.click(saveIn('K9DCS')!)
    expect(memories()[1]).toMatchObject({ callsign: 'K9DCS', toneMode: 'dtcs', dtcsCode: 23, offsetDir: 'plus', offsetMhz: 5, notes: 'Beloit' })
    expect(memories()[1].ctcssEncHz).toBeUndefined()
  })

  it('finds a machine already saved under another reading of it, and saves another machine on the same output and tone', async () => {
    // Saved earlier from hearham's row of W9ABC: the decorated call, 1 kHz off, an FM memory.
    memoriesStore.set({ ...emptyBank(), memories: [
      { id: 'old', name: 'W9ABC 94', kind: 'repeater', rxMhz: 146.941, mode: 'FM', groups: [], favorite: false, source: 'program', callsign: 'W9ABC-R' },
      // Another machine on W9ABC's output and tone: the bank's own frequency+tone key calls it the same channel.
      { id: 'twin', name: 'K9TWN', kind: 'repeater', rxMhz: 146.94, mode: 'FM', ctcssEncHz: 103.5, groups: [], favorite: false, source: 'user', callsign: 'K9TWN' },
    ] })
    const twin = machine('K9TWN', 146.94, { lat: 43.5 })
    const third = machine('W9THR', 146.94, { lat: 41.9 })
    await fetched([LINKED, twin, third])
    expect(badgeIn('W9ABC')?.title).toBe(t('program.row.saved.title', { name: 'W9ABC 94' }))
    expect(saveIn('W9ABC')).toBeNull()
    expect(badgeIn('K9TWN')).not.toBeNull()
    // CONTROL: W9THR shares W9ABC's output and tone, but it is another machine, so it is not marked saved...
    expect(badgeIn('W9THR')).toBeNull()
    // ...and saving it adds a memory of its own.
    fireEvent.click(saveIn('W9THR')!)
    expect(memories().map((m) => m.callsign)).toEqual(['W9ABC-R', 'K9TWN', 'W9THR'])
  })

  it('Save all shown saves every FM machine shown, once each, and leaves the digital-only one and the saved one alone', async () => {
    memoriesStore.set({ ...emptyBank(), memories: [
      { id: 'old', name: 'W9ABC 94', kind: 'repeater', rxMhz: 146.94, mode: 'NFM', groups: [], favorite: true, source: 'program', callsign: 'W9ABC', notes: 'edited by hand' },
    ] })
    // FM only hides the digital machine by default: show it, so Save all shown has it in view.
    await fetched([LINKED, DCS, DMR_ONLY], 2)
    fireEvent.click(screen.getByRole('button', { name: t('program.filters.digital.label') }))
    await waitFor(() => expect(document.querySelectorAll('.rp-results .rp-row').length).toBe(3))
    // The digital-only row has no save at all; Save all shown is offered while an FM row is unsaved.
    expect(saveIn('W9DMR')).toBeNull()
    expect(badgeIn('W9DMR')).toBeNull()
    fireEvent.click(saveAll()!)
    await waitFor(() => expect(memories()).toHaveLength(2))
    expect(memories().map((m) => m.callsign)).toEqual(['W9ABC', 'K9DCS'])
    // The memory that was there is exactly as the operator left it.
    expect(memories()[0]).toMatchObject({ id: 'old', notes: 'edited by hand', favorite: true })
    expect(toasts).toContain(
      t('program.saveBank.done', { count: 1, dupes: t('program.saveBank.dupes', { count: 1 }) }),
    )
    // Everything shown is saved now: the button goes, and a second press could add nothing.
    await waitFor(() => expect(saveAll()).toBeNull())
    expect(badgeIn('K9DCS')).not.toBeNull()
  })
})
