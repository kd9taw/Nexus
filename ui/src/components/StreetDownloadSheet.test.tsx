// @vitest-environment jsdom
//
// The street map's download sheet (operator rulings 2026-10-04: D2 a 50/100/200/400 km square around
// the station or the map centre, exact size first; D3 All streets by default, or Main roads). The
// street-map commands answer at the API boundary. Pinned here: the choices and their defaults, the
// exact size asked for each choice and shown before anything downloads, the free disk and the disk
// refusal, the licence credit, Resume after a restart, progress while it runs, and Retry after a pause.
import { act, cleanup, fireEvent, render, screen, within } from '@testing-library/react'
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import type { StreetProgress, StreetSize, StreetUnfinished } from '../features/streetMaps'

const api = vi.hoisted(() => ({
  size: null as null | Partial<StreetSize>,
  unfinished: [] as StreetUnfinished[],
  asked: [] as unknown[],
  download: null as null | { area: unknown; onProgress: (p: StreetProgress) => void; reject: (e: unknown) => void },
}))
vi.mock('../api', () => ({
  isTauri: () => true,
  streetMapInfo: vi.fn(async () => ({ folder: '/maps', bench: true })),
  streetMapPacks: vi.fn(async () => []),
  streetMapUnfinished: vi.fn(async () => api.unfinished),
  streetMapSize: vi.fn(async (area: { km: number; detail: string }) => {
    api.asked.push(area)
    return {
      packId: 'p',
      bytes: 126_400_000,
      downloadBytes: 130_100_000,
      assetsBytes: 11_600_000,
      tiles: 9000,
      requests: 40,
      bbox: [0, 0, 1, 1],
      minZoom: 0,
      maxZoom: area.detail === 'streets' ? 14 : 12,
      detail: area.detail,
      buildId: '20261004',
      dataDate: '2026-10-04',
      freeBytes: 40_000_000_000,
      enoughSpace: true,
      resumeBytes: 0,
      installed: false,
      ...api.size,
    }
  }),
  streetMapDownload: vi.fn(
    (area: unknown, onProgress: (p: StreetProgress) => void) =>
      new Promise((_resolve, reject) => {
        api.download = { area, onProgress, reject }
      }),
  ),
  streetMapCancel: vi.fn(async () => true),
  streetMapRemove: vi.fn(async () => 0),
}))
vi.mock('../gpu', () => ({ webgl2Available: () => true }))
import { StreetDownloadSheet } from './StreetDownloadSheet'
import { __resetStreetMapsForTests, gb, mb } from '../features/streetMaps'
import { t } from '../i18n'

const SALINA = { lat: 38.84, lon: -97.61 }

async function settle(ms = 0) {
  await act(async () => {
    if (ms) await new Promise((r) => setTimeout(r, ms))
  })
  for (let i = 0; i < 3; i++) await act(async () => {})
}
async function open(over: Partial<Parameters<typeof StreetDownloadSheet>[0]> = {}) {
  const onInstalled = vi.fn()
  render(<StreetDownloadSheet open onClose={() => {}} myGrid="EM18eu" mapCentre={SALINA} onInstalled={onInstalled} {...over} />)
  await settle(300)
  return { onInstalled, sheet: screen.getByRole('dialog', { name: t('map.street.sheet.title') }) }
}
const group = (name: string) => screen.getByRole('group', { name })
const chip = (g: string, name: string) => within(group(g)).getByRole('button', { name: new RegExp(`^${name}`) })
const pressedIn = (g: string) =>
  within(group(g))
    .getAllByRole('button')
    .filter((b) => b.getAttribute('aria-pressed') === 'true')
    .map((b) => b.textContent)
const downloadButton = () => screen.getByRole('button', { name: t('map.street.sheet.download') })
const lastAsked = () => api.asked[api.asked.length - 1]

beforeEach(() => {
  __resetStreetMapsForTests()
  Object.assign(api, { size: null, unfinished: [], asked: [], download: null })
})
afterEach(() => cleanup())

describe('the download sheet — the three choices', () => {
  it('offers 50, 100, 200 and 400 km with 200 chosen, All streets chosen, around the station', async () => {
    await open()
    expect(within(group(t('map.street.sheet.size'))).getAllByRole('button').map((b) => b.textContent)).toEqual([
      '50 km',
      '100 km',
      '200 km',
      '400 km',
    ])
    expect(pressedIn(t('map.street.sheet.size'))).toEqual(['200 km'])
    expect(pressedIn(t('map.street.sheet.detail'))).toEqual([t('map.street.sheet.detail.streets')])
    expect(pressedIn(t('map.street.sheet.where'))[0]).toContain(t('map.street.sheet.where.station'))
    expect(lastAsked()).toMatchObject({ km: 200, detail: 'streets' })
  })

  it('asks the exact size again for every choice, and shows it before anything downloads', async () => {
    await open()
    expect(screen.getByText(t('map.street.sheet.measure.sizeAssets', { size: mb(130_100_000), assets: mb(11_600_000) }))).toBeTruthy()
    expect(screen.getByText(t('map.street.sheet.measure.free', { free: gb(40_000_000_000) }))).toBeTruthy()
    fireEvent.click(chip(t('map.street.sheet.size'), '50 km'))
    fireEvent.click(chip(t('map.street.sheet.detail'), t('map.street.sheet.detail.roads')))
    await settle(300)
    expect(lastAsked()).toMatchObject({ km: 50, detail: 'roads' })
    fireEvent.click(chip(t('map.street.sheet.where'), t('map.street.sheet.where.centre')))
    await settle(300)
    expect(lastAsked()).toMatchObject({ lat: SALINA.lat, lon: SALINA.lon, km: 50 })
    expect(api.download, 'nothing downloads until Download is pressed').toBeNull()
  })

  it("leaves the map's centre out where there is no map (Settings), and the station out without a grid", async () => {
    await open({ mapCentre: null })
    expect(chip(t('map.street.sheet.where'), t('map.street.sheet.where.centre')).hasAttribute('disabled')).toBe(true)
    cleanup()
    await open({ myGrid: '', mapCentre: null })
    expect(chip(t('map.street.sheet.where'), t('map.street.sheet.where.station')).hasAttribute('disabled')).toBe(true)
    expect(screen.getByText(t('map.street.sheet.where.noGrid'))).toBeTruthy()
    expect(downloadButton().hasAttribute('disabled')).toBe(true)
  })

  it('carries the licence credit, © OpenStreetMap linked to its copyright page', async () => {
    const { sheet } = await open()
    const a = within(sheet).getByRole('link', { name: '© OpenStreetMap' })
    expect(a.getAttribute('href')).toBe('https://www.openstreetmap.org/copyright')
  })
})

describe('the download sheet — refusals and states', () => {
  it('refuses where the disk has less than twice the download free, saying how much it needs', async () => {
    api.size = { enoughSpace: false, freeBytes: 150_000_000, assetsBytes: 0 }
    await open()
    expect(downloadButton().hasAttribute('disabled')).toBe(true)
    expect(screen.getByText(t('map.street.sheet.measure.noSpace', { need: gb(2 * 130_100_000), free: gb(150_000_000) }))).toBeTruthy()
  })

  it('has nothing to download for a map already on this computer', async () => {
    api.size = { installed: true }
    await open()
    expect(screen.getByText(t('map.street.sheet.measure.installed'))).toBeTruthy()
    expect(downloadButton().hasAttribute('disabled')).toBe(true)
  })

  it('downloads, shows how far it has got, and can be cancelled', async () => {
    await open()
    fireEvent.click(downloadButton())
    await settle()
    expect(api.download?.area).toMatchObject({ km: 200, detail: 'streets' })
    act(() => api.download!.onProgress({ phase: 'tiles', done: 65_050_000, total: 130_100_000, bytesPerSec: 2_000_000, etaSecs: 33 }))
    expect(screen.getByRole('progressbar').getAttribute('value')).toBe('50')
    expect(
      screen.getByText(t('map.street.sheet.running.tilesEta', { done: mb(65_050_000), total: mb(130_100_000), speed: mb(2_000_000), mins: 1 })),
    ).toBeTruthy()
    expect(screen.getByRole('button', { name: t('map.street.sheet.stop') })).toBeTruthy()
  })

  it('offers Retry after a pause, and Retry resumes the same square', async () => {
    await open()
    fireEvent.click(downloadButton())
    await settle()
    const first = api.download!
    await act(async () => {
      first.reject({ kind: 'paused', message: 'retries ran out' })
    })
    await settle()
    expect(screen.getByText(t('map.street.sheet.stopped', { reason: t('map.street.error.paused') }))).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: t('map.street.sheet.retry') }))
    await settle()
    expect(api.download).not.toBe(first)
    expect(api.download!.area).toEqual(first.area)
  })

  it('offers to resume a download that stopped before a restart', async () => {
    api.unfinished = [
      {
        packId: '20261004-streets-200km-3884n09761w',
        area: { lat: 38.84, lon: -97.61, km: 200, detail: 'streets' },
        buildId: '20261004',
        dataDate: '2026-10-04',
        doneBytes: 62_000_000,
        totalBytes: 126_000_000,
      },
    ]
    await open()
    expect(screen.getByText(t('map.street.sheet.unfinished', { name: 'EM18eu 200 km', done: mb(62_000_000), total: mb(126_000_000) }))).toBeTruthy()
    fireEvent.click(screen.getByRole('button', { name: t('map.street.sheet.resume') }))
    await settle()
    expect(api.download?.area).toEqual(api.unfinished[0].area)
  })
})
