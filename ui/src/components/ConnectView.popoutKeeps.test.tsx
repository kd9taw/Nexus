// @vitest-environment jsdom
//
// A POP-OUT KEEPS WHAT IT SHOWS (operator report, 2026-10-04): "After running Conditions all day on a
// separate monitor, it all of a sudden shows every satellite ever launched - but only on the popped out
// screen, not on the main screen ... I never turned them on to begin with."
//
// The dashboard window (`?panel=connect`) borrows the main window's stored values until it writes its
// own (features/windowScope `surfaceGet`). Its map layers became its own on its first open, but the
// Conditions INTENT never did, and its first write copied EVERY intent's map setup from the main window
// as it stood that day. So a reload or a reopen, with nothing pressed in it, put the pop-out on the
// intent the main window had moved to and showed that intent's copied setup — satellites the main
// window had since turned off included. Each test below opens the two windows in turn over one
// storage, as the app's windows share one WebView data folder, and presses nothing in the pop-out
// unless it says so.
//
// Drives the REAL ConnectView + REAL MapView. Globe3D is stubbed only so a test can see which map
// mounted (jsdom has no WebGL), and the GPU probe answers "capable".
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, render, fireEvent, screen, act, within } from '@testing-library/react'

vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  getBandOutlook: vi.fn(async () => ({ bands: [], asOf: 0 })),
  getGettingOut: vi.fn(async () => null),
  getPathOutlook: vi.fn(async () => null),
  getSpaceWxScales: vi.fn(async () => ({ scales: null, alerts: [] })),
  getKc2gMuf: vi.fn(async () => []),
  getXrayNow: vi.fn(async () => null),
  getDxpedWindows: vi.fn(async () => []),
  getAurora: vi.fn(async () => null),
  getDeclination: vi.fn(async () => null),
  getPca: vi.fn(async () => null),
  getSatellites: vi.fn(async () => null),
  getLogStats: vi.fn(async () => null),
  getOtaMapSpots: vi.fn(async () => []),
}))
vi.mock('./Globe3D', () => ({ default: () => <div data-testid="globe3d-stub" /> }))
vi.mock('../gpu', () => ({ gpuCapableForGlobe: () => true }))
import { ConnectView } from './ConnectView'
import { pastTheSwitch } from './ConnectView.testkit'

class RO {
  observe() {}
  unobserve() {}
  disconnect() {}
}
;(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = RO

const props = {
  myGrid: 'EN52',
  theme: 'dark' as const,
  stations: [],
  prop: null,
  selectedCall: null,
  onSelectCall: () => {},
  needByCall: new Map(),
  needAlerts: [],
  amp: null,
}

const SATS = 'Satellites (amateur)'

/** Open Conditions in a window: the main window, or the dashboard pop-out. Closing it (cleanup) and
 *  opening it again is a reload, or the window closed and reopened. */
async function open(which: 'main' | 'popout') {
  window.history.replaceState(null, '', which === 'main' ? '/' : '/?panel=connect')
  await act(async () => {
    render(<ConnectView {...props} />)
  })
}
const sats = () => (screen.getByLabelText(SATS) as HTMLInputElement).checked
const tick = (name: string) => fireEvent.click(screen.getByLabelText(name))
const intent = (name: string) => fireEvent.click(screen.getByRole('button', { name }))
/** The intent the window shows, as its button reads. */
const shownIntent = () => document.querySelector('.connect-intent button.active')?.textContent ?? null
const picker = () => screen.getByRole('group', { name: 'Map view' })
const choose = (name: string) => fireEvent.click(within(picker()).getByRole('button', { name }))
const shownMap = () =>
  within(picker())
    .getAllByRole('button')
    .filter((b) => b.getAttribute('aria-pressed') === 'true')
    .map((b) => b.textContent)

describe('a pop-out keeps what it shows across its own reload', () => {
  beforeEach(() => {
    localStorage.clear()
    pastTheSwitch('main', 'connect')
  })
  afterEach(() => cleanup())

  it('reopened after the main window moved to another intent, it keeps its intent and turns no layer on', async () => {
    // The main window, some time ago: Ragchew with the satellites ticked, then back to Chase DX.
    await open('main')
    intent('Ragchew')
    tick(SATS)
    intent('Chase DX')
    expect(sats(), 'CONTROL: the main window shows no satellites').toBe(false)
    cleanup()

    // The dashboard window opens on what the main window shows: Chase DX, no satellites.
    await open('popout')
    expect(shownIntent(), 'CONTROL: it opened on the main window’s intent').toBe('Chase DX')
    expect(sats(), 'CONTROL: no satellites in the pop-out').toBe(false)
    cleanup()

    // Later, in the main window: Ragchew again, and the satellites unticked there.
    await open('main')
    intent('Ragchew')
    tick(SATS)
    expect(sats(), 'CONTROL: the main window shows no satellites on Ragchew either').toBe(false)
    cleanup()

    // The dashboard window reloads. Nothing is pressed in it.
    await open('popout')
    expect(shownIntent(), 'the pop-out followed the main window to another intent').toBe('Chase DX')
    expect(sats(), 'a reload turned the satellites on in the pop-out').toBe(false)
  })

  it('its first open keeps only the setup it shows: another intent’s old setup is not copied from the main window', async () => {
    // The main window ticked the satellites on Ragchew and is on Chase DX.
    await open('main')
    intent('Ragchew')
    tick(SATS)
    intent('Chase DX')
    cleanup()
    // The pop-out opens (Chase DX), then the main window unticks the satellites on Ragchew.
    await open('popout')
    cleanup()
    await open('main')
    intent('Ragchew')
    tick(SATS)
    expect(sats(), 'CONTROL: Ragchew shows no satellites in the main window').toBe(false)
    cleanup()

    // In the pop-out the operator picks Ragchew, for the first time there.
    await open('popout')
    intent('Ragchew')
    expect(shownIntent(), 'CONTROL: the pick landed').toBe('Ragchew')
    expect(sats(), 'the pop-out showed a copy of the main window’s old Ragchew setup').toBe(false)
  })

  it('on the 3-D globe it keeps its map when the main window changes its own', async () => {
    // The main window shows Chase DX on the 3-D globe.
    await open('main')
    choose('3D')
    expect(await screen.findByTestId('globe3d-stub'), 'CONTROL: the main window is on 3D').toBeTruthy()
    cleanup()
    // The pop-out opens on the same: the 3-D globe.
    await open('popout')
    expect(await screen.findByTestId('globe3d-stub'), 'CONTROL: the pop-out opened on 3D').toBeTruthy()
    cleanup()
    // The main window moves its Chase DX map to Beam.
    await open('main')
    choose('Beam')
    expect(shownMap(), 'CONTROL').toEqual(['Beam'])
    cleanup()
    // The pop-out reloads.
    await open('popout')
    expect(shownMap(), 'the pop-out followed the main window’s map').toEqual(['3D'])
  })

  it('keeps its ★/All choice when the main window flips its own', async () => {
    // The operator ticks the satellites in the pop-out; its ★/All reads ★ (the default).
    await open('popout')
    tick(SATS)
    const chip = () => screen.getByRole('button', { name: 'Filter satellites to ★ birds' })
    expect(chip().textContent, 'CONTROL: ★ in the pop-out').toBe('★')
    cleanup()
    // The main window shows all satellites.
    await open('main')
    tick(SATS)
    fireEvent.click(chip())
    expect(chip().textContent, 'CONTROL: All in the main window').toBe('All')
    cleanup()
    // The pop-out reloads.
    await open('popout')
    expect(sats(), 'CONTROL: its own satellites tick kept').toBe(true)
    expect(chip().textContent, 'the pop-out followed the main window’s ★/All').toBe('★')
  })

  it('the main window writes nothing it only read', async () => {
    await open('main')
    expect(
      Object.keys(localStorage).filter((k) => k === 'nexus.connect.intent' || k === 'nexus.sats.favOnly'),
      'the main window wrote a value it only read',
    ).toEqual([])
  })
})
