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
// mounted (jsdom has no WebGL), and the GPU probe is stubbed so both of its answers can be driven.
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
const gpu = { ok: true }
vi.mock('../gpu', () => ({ gpuCapableForGlobe: () => gpu.ok }))
import { ConnectView } from './ConnectView'
import { DEFAULT_LAYERS } from './MapView'
import { pastTheSwitch } from './ConnectView.testkit'

// THE BUDGET (2026-10-09). The slowest case here, "reopened after the main window moved to another intent…", takes
// 0.89 s and 0.81 s on one core (two runs), nearly all of it CPU work (3.05 s at a third of a CPU); a loaded full
// suite on this box has run cases up to 20 times slower than one core, 16.2 s for this one. 20 s holds that; a test
// that hangs still fails, after 20 s.
vi.setConfig({ testTimeout: 20_000 })

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
    gpu.ok = true
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

// WHEN THE 3-D GLOBE CANNOT DRAW AT A RELOAD (the operator, 2026-10-04: "once I got rid of the
// satellites, I noticed that the state outline had appeared as well" … "here's the 3d with no
// satellites. but when I switch to flat, I get the satellites again"). Nothing switches the map in a
// running window: whether the 3-D globe can draw is asked once, when Conditions opens. A reload or a
// reopen that finds the graphics unable to (a GPU fallen back to software, a remote desktop) shows the
// 2-D Globe for a stored 3D pick, and said so only in the 3D button's tooltip. The 2-D layers it then
// showed were ones nobody chose in that window: the main window's, taken over with the 3D pick, and
// with them the satellites a Frame tap on the main window's 3-D globe had ticked into its 2-D record.
describe('a pop-out on the 3-D globe that opens on the 2-D map', () => {
  const STATES = 'US states'
  const states = () => (screen.getByLabelText(STATES) as HTMLInputElement).checked
  const note = () => document.querySelector('.connect-map-note')

  beforeEach(() => {
    localStorage.clear()
    gpu.ok = true
    pastTheSwitch('main', 'connect')
    // The main window: Chase DX on the 3-D globe, and in its 2-D record what a Frame tap there left
    // before Frame ticked only the map on screen: the satellites on, beside the 2-D defaults.
    localStorage.setItem(
      'nexus.connect.intents',
      JSON.stringify({ dx: { map: '3d', layers: { ...DEFAULT_LAYERS, sats: { visible: true, opacity: 0.9 } } } }),
    )
  })
  afterEach(() => cleanup())

  it('shows the layers chosen on this window’s globe, not satellites and state outlines nobody chose here', async () => {
    await open('popout')
    expect(await screen.findByTestId('globe3d-stub'), 'CONTROL: the pop-out opened on 3D').toBeTruthy()
    // What the operator chose on this window's 3-D globe (Globe3D keeps its picks per window).
    localStorage.setItem(
      'nexus.connect.globe3d.layers.connect',
      JSON.stringify({ spots: true, arcs: true, states: false, sats: false, heat: true, rings: true }),
    )
    cleanup()
    // A reload that finds the graphics unable to run the 3-D globe.
    gpu.ok = false
    await open('popout')
    expect(screen.queryByTestId('globe3d-stub'), 'CONTROL: the 2-D map stands in').toBeNull()
    expect(sats(), 'satellites nobody chose in this window').toBe(false)
    expect(states(), 'state outlines this window’s operator turned off').toBe(false)
  })

  it('says why the map changed, for as long as the 2-D map stands in for 3D', async () => {
    await open('popout')
    expect(await screen.findByTestId('globe3d-stub'), 'CONTROL').toBeTruthy()
    expect(note(), 'CONTROL: nothing to say while 3D draws').toBeNull()
    cleanup()
    gpu.ok = false
    await open('popout')
    expect(note()?.textContent, 'the map changed with nothing said').toMatch(/3D/)
    expect(shownMap(), 'CONTROL: the 2-D Globe stands in').toEqual(['Globe'])
    // Picking a 2-D map is the operator's own choice: nothing stands in, so nothing to say.
    choose('Flat')
    expect(note()).toBeNull()
  })

  it('the main window says so too', async () => {
    gpu.ok = false
    await open('main')
    expect(note()?.textContent).toMatch(/3D/)
  })
})
