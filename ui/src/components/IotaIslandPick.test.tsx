// @vitest-environment jsdom
//
// THE ISLAND WITHIN A GROUP (operator ruling, 2026-11-10): IOTA_ISLAND_ID is only ever the operator's
// pick. With a group in a form's IOTA box, the picker lists that group's islands (and only that
// group's) from the station's islands list; nothing is picked until the operator picks, a new group
// lets go of the pick, and the contact is logged with the island id only when one is picked.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, fireEvent, cleanup, waitFor } from '@testing-library/react'
import { LogConfirm } from './LogConfirm'
import { IotaIslandPick } from './Iota'
import { resetIotaGroupsForTest, withIotaIsland } from '../features/iotaGroups'
import { cardIsland, cardSave, draftOf, fieldsOf, pickCardIsland } from '../features/contactCard'
import { t } from '../i18n'
import type { IotaIsland } from '../api'
import type { LoggedQso } from '../types'

const ISLANDS: Record<string, IotaIsland[]> = {
  'EU-028': [
    { id: 101, name: 'Alpha Test Island' },
    { id: 102, name: 'Beta Isle', comment: 'in Example Land' },
  ],
  'EU-001': [{ id: 201, name: 'First Isle' }],
}

const api = vi.hoisted(() => ({ getIotaOffer: vi.fn(), getIotaIslands: vi.fn() }))
vi.mock('../api', async (orig) => ({
  ...(await orig<typeof import('../api')>()),
  getIotaOffer: api.getIotaOffer,
  getIotaIslands: api.getIotaIslands,
}))

const GROUPS = [
  { reference: 'EU-028', name: 'Example Isles group' },
  { reference: 'EU-001', name: 'First Isles group' },
]
const status = { enabled: true, count: 2, fetchedAt: 0, bundled: true, lastError: null }

beforeEach(() => {
  vi.clearAllMocks()
  api.getIotaOffer.mockResolvedValue(null)
  api.getIotaIslands.mockImplementation(async (r: string) => ISLANDS[r] ?? [])
  resetIotaGroupsForTest(GROUPS, status)
})
afterEach(() => {
  cleanup()
  resetIotaGroupsForTest()
})

const picker = () => screen.queryByRole('combobox', { name: t('iota.island.label') }) as HTMLSelectElement | null
const options = () => [...(picker()?.options ?? [])].map((o) => o.textContent)

describe('the island picker', () => {
  it("lists only the group's own islands, nothing picked, and none for a group with none", async () => {
    const { rerender } = render(<IotaIslandPick reference="eu-028" value={null} onChange={() => {}} />)
    await waitFor(() => expect(picker()).toBeTruthy())
    expect(api.getIotaIslands).toHaveBeenCalledWith('EU-028')
    expect(options()).toEqual([t('iota.island.none'), 'Alpha Test Island', 'Beta Isle (in Example Land)'])
    expect(picker()!.value, 'nothing picked').toBe('')
    rerender(<IotaIslandPick reference="EU-001" value={null} onChange={() => {}} />)
    await waitFor(() => expect(options()).toEqual([t('iota.island.none'), 'First Isle']))
    rerender(<IotaIslandPick reference="NA-999" value={null} onChange={() => {}} />)
    await waitFor(() => expect(picker()).toBeNull())
    rerender(<IotaIslandPick reference="" value={null} onChange={() => {}} />)
    expect(picker()).toBeNull()
  })

  it('shows nothing without the islands list', async () => {
    api.getIotaIslands.mockResolvedValue([])
    render(<IotaIslandPick reference="EU-028" value={null} onChange={() => {}} />)
    await waitFor(() => expect(api.getIotaIslands).toHaveBeenCalled())
    expect(picker()).toBeNull()
  })
})

const QSO = {
  call: 'IA5X',
  grid: 'JN52',
  band: '20m',
  mode: 'FT8',
  rstSent: '-12',
  rstRcvd: '-09',
  comment: null,
  txPower: null,
  operator: null,
  myGrid: null,
  propMode: null,
  ota: { iota: 'EU-028' },
} as unknown as LoggedQso

function open(record: LoggedQso = QSO) {
  const onConfirm = vi.fn()
  render(<LogConfirm record={record} onConfirm={onConfirm} onDiscard={() => {}} />)
  return {
    island: () => (onConfirm.mock.calls[0][0] as LoggedQso).extra?.find(([k]) => k === 'IOTA_ISLAND_ID')?.[1] ?? null,
    log: () => fireEvent.click(screen.getByRole('button', { name: t('logPrompt.log') })),
  }
}

const box = () => screen.getByTitle(t('iota.prefill.hint')) as HTMLInputElement

describe('the popup logs an island only when one is picked', () => {
  it('a picked island logs its id', async () => {
    const p = open()
    await waitFor(() => expect(picker()).toBeTruthy())
    fireEvent.change(picker()!, { target: { value: '102' } })
    p.log()
    expect(p.island()).toBe('102')
  })

  it('no pick logs none', async () => {
    const p = open()
    await waitFor(() => expect(picker()).toBeTruthy())
    p.log()
    expect(p.island()).toBeNull()
  })

  it('a new group lets go of the pick, and lists its own islands', async () => {
    const p = open()
    await waitFor(() => expect(picker()).toBeTruthy())
    fireEvent.change(picker()!, { target: { value: '101' } })
    fireEvent.change(box(), { target: { value: 'EU-001' } })
    await waitFor(() => expect(options()).toEqual([t('iota.island.none'), 'First Isle']))
    expect(picker()!.value).toBe('')
    p.log()
    expect(p.island()).toBeNull()
  })

  it('a cleared reference logs no island', async () => {
    const p = open()
    await waitFor(() => expect(picker()).toBeTruthy())
    fireEvent.change(picker()!, { target: { value: '101' } })
    fireEvent.click(screen.getByRole('button', { name: t('iota.offer.dismiss') }))
    p.log()
    expect(p.island()).toBeNull()
  })
})

describe("the contact card's island", () => {
  const stored = { ...QSO, extra: [['IOTA_ISLAND_ID', '101'], ['CQZ', '15']] } as unknown as LoggedQso

  it('a pick saves as the field it is, a removal too, and other fields are kept', () => {
    const fields = fieldsOf(stored)
    expect(cardIsland(fields)).toBe(101)
    const d = draftOf(stored)
    const repicked = cardSave(stored, d, d, pickCardIsland(fields, 102))
    expect(repicked.ok && repicked.fields).toEqual([{ tag: 'IOTA_ISLAND_ID', old: '101', new: '102' }])
    const none = cardSave(stored, d, d, pickCardIsland(fields, null))
    expect(none.ok && none.fields).toEqual([{ tag: 'IOTA_ISLAND_ID', old: '101', new: null }])
    const bare = { ...QSO, extra: [] } as unknown as LoggedQso
    const added = cardSave(bare, draftOf(bare), draftOf(bare), pickCardIsland(fieldsOf(bare), 201))
    expect(added.ok && added.fields).toEqual([{ tag: 'IOTA_ISLAND_ID', old: null, new: '201' }])
    expect(withIotaIsland([['CQZ', '15'], ['IOTA_ISLAND_ID', '9']], null)).toEqual([['CQZ', '15']])
  })

  it('a new group takes off the island the contact had, unless one is picked again', () => {
    const d = draftOf(stored)
    const moved = { ...d, iota: 'EU-001' }
    const fields = fieldsOf(stored)
    const dropped = cardSave(stored, d, moved, fields)
    expect(dropped.ok && dropped.fields).toEqual([{ tag: 'IOTA_ISLAND_ID', old: '101', new: null }])
    const repicked = cardSave(stored, d, moved, pickCardIsland(fields, 201))
    expect(repicked.ok && repicked.fields).toEqual([{ tag: 'IOTA_ISLAND_ID', old: '101', new: '201' }])
  })
})
