// @vitest-environment jsdom
// A received picture can be saved from the browser: the link hands over the already verified
// image this page is showing. Nothing reaches the station, and the desktop (whose gallery is
// already on disk) is unchanged.
import { afterEach, expect, it, vi } from 'vitest'
import { cleanup, render, waitFor } from '@testing-library/react'
import { SstvView } from './SstvView'
import { getSstvState } from '../api'
import { useSstvImage } from '../remote-web/useSstvImage'
import { parseSstvSample } from '../remote-web/sstv'
import { RemoteCollectionsContext, type RemoteCollections } from '../remote-web/collections'
import { StationDataContext } from '../stationAccess'
import sstvFixture from '../remote-web/__fixtures__/sstv.json'
import type { AppSnapshot } from '../types'

vi.mock('./Waterfall', () => ({ Waterfall: () => null }))
vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  sstvArm: vi.fn(),
  sstvAutoArm: vi.fn(async () => null),
  getLicensedBandPlan: vi.fn(async () => []),
  sstvSend: vi.fn(),
  sstvStop: vi.fn(),
  sstvDeleteImage: vi.fn(),
  setOperatingMode: vi.fn(),
  setRfPower: vi.fn(async () => {}),
}))
vi.mock('../remote-web/useSstvImage', () => ({ useSstvImage: vi.fn() }))
afterEach(() => { cleanup(); vi.clearAllMocks() })

const snap = { mycall: 'N0CALL', radio: { dialMhz: 14.23, band: '20m', catOk: true, sideband: 'USB', transmitting: false, txEnabled: false, tuning: false, txAllowed: true } } as unknown as AppSnapshot
const gallery = (sstvFixture as { state: { gallery: { path: string }[] } }).state.gallery
function remote(url: string | null) {
  vi.mocked(getSstvState).mockImplementation(async () => parseSstvSample(structuredClone(sstvFixture), 0))
  vi.mocked(useSstvImage).mockReturnValue({ url, failed: false, retry: vi.fn() })
  const source = { client: { supports: () => true } } as unknown as RemoteCollections
  return render(<StationDataContext.Provider value={true}><RemoteCollectionsContext.Provider value={source}>
    <SstvView snap={snap} active />
  </RemoteCollectionsContext.Provider></StationDataContext.Provider>)
}

it('offers each loaded station picture as a download of the verified image, named without the station path', async () => {
  expect(gallery.length).toBeGreaterThan(0)
  const view = remote('blob:nexus-station-image')
  await waitFor(() => expect(view.container.querySelectorAll('.sstv-thumb')).toHaveLength(gallery.length))
  const links = [...view.container.querySelectorAll<HTMLAnchorElement>('a.sstv-thumb-save')]
  expect(links).toHaveLength(gallery.length)
  for (const [i, link] of links.entries()) {
    expect(link.getAttribute('href')).toBe('blob:nexus-station-image')
    const name = link.getAttribute('download')!
    expect(name).toMatch(/^nexus-sstv-[A-Za-z0-9._-]+\.(png|bmp)$/)
    expect(name.endsWith(gallery[i].path.slice(-4))).toBe(true)
    expect(name).not.toContain(gallery[i].path.slice(0, 8))
    expect(link.getAttribute('aria-label')).toBeTruthy()
  }
  // The reserved row becomes the real link — it is never both, so the card keeps its height.
  expect(view.container.querySelector('.sstv-thumb-save-slot')).toBeNull()
})

it('offers nothing to save until the picture has loaded, and nothing on the desktop', async () => {
  const view = remote(null)
  await waitFor(() => expect(view.container.querySelectorAll('.sstv-thumb')).toHaveLength(gallery.length))
  expect(view.container.querySelector('.sstv-thumb-save')).toBeNull()
  // …but the row it will occupy is already there, or the card grows when the picture lands and
  // shoves the rest of the gallery down mid-scroll (the remote browser suite measured 27.4 px).
  // The placeholder carries `.cw-macro`, the link's own box, so the height matches at any zoom.
  const slots = [...view.container.querySelectorAll('.sstv-thumb-save-slot')]
  expect(slots).toHaveLength(gallery.length)
  for (const slot of slots) {
    expect(slot.classList.contains('cw-macro')).toBe(true)
    expect(slot.textContent).toBe('Save')
    expect(slot.getAttribute('aria-hidden')).toBe('true')
  }
  cleanup()
  vi.mocked(getSstvState).mockResolvedValue(parseSstvSample(structuredClone(sstvFixture), 0))
  const desktop = render(<SstvView snap={snap} active />)
  await waitFor(() => expect(desktop.container.querySelectorAll('.sstv-thumb')).toHaveLength(gallery.length))
  expect(desktop.container.querySelector('.sstv-thumb-save')).toBeNull()
  expect(desktop.container.querySelector('.sstv-thumb-save-slot')).toBeNull()
})
