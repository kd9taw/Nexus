// @vitest-environment jsdom
//
// Settings ▸ Appearance ▸ Map & globe ▸ Street maps (operator ruling 2026-10-04, D5: Update and Remove
// live in Settings; D8: the app offers an update and never downloads one by itself). The street-map
// commands answer at the API boundary. Pinned here: the block exists only while the street map is
// offered; each installed pack is listed with its area, detail, size and data date, and the folder for
// removing it by hand; Remove asks first and says the space freed; Check for updates asks the host only
// when pressed, and Update downloads only when pressed; Download another area opens the sheet; and the
// bench aid installs a local file.
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { StreetPack } from '../features/streetPack'
import type { StreetUpdate } from '../features/streetMaps'

const PACK: StreetPack = {
  id: '20261004-streets-200km-3884n09761w',
  name: 'EM18eu 200 km',
  bbox: [-98.8, 37.9, -96.5, 39.8],
  minZoom: 0,
  maxZoom: 14,
  detail: 'streets',
  bytes: 126_400_000,
  dataDate: '2026-10-04',
  sha256: 'ab'.repeat(32),
}
const api = vi.hoisted(() => ({
  tauri: true,
  bench: true,
  packs: [] as unknown[],
  updates: [] as unknown[],
  downloaded: [] as unknown[],
  installed: null as unknown,
  confirm: true,
}))
vi.mock('../api', () => ({
  isTauri: () => api.tauri,
  streetMapInfo: vi.fn(async () => ({ folder: 'C:\\Users\\op\\AppData\\Local\\Nexus\\maps', bench: api.bench })),
  streetMapPacks: vi.fn(async () => api.packs),
  streetMapUnfinished: vi.fn(async () => []),
  streetMapSize: vi.fn(() => new Promise(() => {})),
  streetMapDownload: vi.fn((area: unknown) => {
    api.downloaded.push(area)
    return new Promise(() => {})
  }),
  streetMapCancel: vi.fn(async () => true),
  streetMapRemove: vi.fn(async (id: string) => {
    api.packs = api.packs.filter((p) => (p as StreetPack).id !== id)
    return 126_400_000
  }),
  streetMapUpdates: vi.fn(async () => api.updates),
  streetMapInstallFile: vi.fn(async () => api.installed),
}))
vi.mock('../gpu', () => ({ webgl2Available: () => true }))
vi.mock('../confirm', () => ({ confirmDialog: vi.fn(async () => api.confirm) }))
import { SettingsStreetMaps } from './SettingsStreetMaps'
import { __resetStreetMapsForTests, mb } from '../features/streetMaps'
import { confirmDialog } from '../confirm'
import { streetMapInstallFile, streetMapRemove, streetMapUpdates } from '../api'
import { t } from '../i18n'

async function settle() {
  for (let i = 0; i < 4; i++) await act(async () => {})
}
async function mount() {
  const view = render(<SettingsStreetMaps myGrid="EM18eu" />)
  await settle()
  return view
}
const button = (name: string) => screen.getByRole('button', { name })

beforeEach(() => {
  __resetStreetMapsForTests()
  Object.assign(api, { tauri: true, bench: true, packs: [PACK], updates: [], downloaded: [], installed: null, confirm: true })
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('Settings ▸ Street maps', () => {
  it('is not there at all while the street map is not offered (outside the desktop shell)', async () => {
    api.tauri = false
    const { container } = await mount()
    expect(container.innerHTML).toBe('')
  })

  it('lists each pack with its area, detail, size and data date, and names the folder', async () => {
    await mount()
    const row = screen.getByText('EM18eu 200 km').closest('li')!
    expect(within(row).getByText(t('settings.streetMaps.pack', { detail: t('map.street.sheet.detail.streets'), size: mb(126_400_000), date: '2026-10-04' }))).toBeTruthy()
    expect(screen.getByText('C:\\Users\\op\\AppData\\Local\\Nexus\\maps').tagName).toBe('CODE')
    expect(screen.getByRole('link', { name: '© OpenStreetMap' }).getAttribute('href')).toBe('https://www.openstreetmap.org/copyright')
  })

  it('removes a pack only after asking, and says the space freed', async () => {
    await mount()
    api.confirm = false
    fireEvent.click(button(t('settings.streetMaps.remove')))
    await settle()
    expect(streetMapRemove, 'a "no" removes nothing').not.toHaveBeenCalled()

    api.confirm = true
    fireEvent.click(button(t('settings.streetMaps.remove')))
    await settle()
    expect(confirmDialog).toHaveBeenLastCalledWith({
      title: t('settings.streetMaps.remove.title', { name: 'EM18eu 200 km' }),
      body: t('settings.streetMaps.remove.body', { size: mb(126_400_000) }),
      confirmLabel: t('settings.streetMaps.remove.confirm'),
      danger: true,
    })
    expect(streetMapRemove).toHaveBeenCalledWith(PACK.id)
    expect(screen.getByText(t('settings.streetMaps.removed', { name: 'EM18eu 200 km', size: mb(126_400_000) }))).toBeTruthy()
    expect(screen.getByText(t('settings.streetMaps.none'))).toBeTruthy()
  })

  it('asks the host about updates only when pressed, and downloads one only when pressed', async () => {
    const u: StreetUpdate = { packId: PACK.id, area: { lat: 38.84, lon: -97.61, km: 200, detail: 'streets' }, dataDate: '2026-10-04', newBuildId: '20270104', newDataDate: '2027-01-04' }
    api.updates = [u]
    await mount()
    expect(streetMapUpdates, 'opening Settings asks the host nothing').not.toHaveBeenCalled()
    fireEvent.click(button(t('settings.streetMaps.updates.check')))
    await settle()
    expect(streetMapUpdates).toHaveBeenCalledTimes(1)
    expect(api.downloaded, 'finding an update downloads nothing').toEqual([])
    fireEvent.click(button(t('settings.streetMaps.update', { date: '2027-01-04' })))
    await settle()
    expect(api.downloaded).toEqual([u.area])
  })

  it('says so when every pack is current', async () => {
    await mount()
    fireEvent.click(button(t('settings.streetMaps.updates.check')))
    await settle()
    expect(screen.getByText(t('settings.streetMaps.updates.none'))).toBeTruthy()
  })

  it('opens the download sheet for another area', async () => {
    await mount()
    fireEvent.click(button(t('settings.streetMaps.another')))
    await settle()
    expect(screen.getByRole('dialog', { name: t('map.street.sheet.title') })).toBeTruthy()
  })

  it('installs a map file on a bench run, through the OS picker in the shell', async () => {
    api.installed = { kind: 'pack', pack: { ...PACK, id: 'local-0123456789ab', name: 'EM18eu 37 km' } }
    await mount()
    fireEvent.click(button(t('settings.streetMaps.bench.install')))
    await settle()
    expect(streetMapInstallFile).toHaveBeenCalledWith()
    expect(screen.getByText(t('settings.streetMaps.bench.installed.pack', { name: 'EM18eu 37 km' }))).toBeTruthy()
  })
})
