// @vitest-environment jsdom
// THE RECEIVED-PICTURE VIEWER — what it shows, how it steps, and how it CLOSES.
//
// The close half is the one worth being careful about. #263 was the torn-off waterfall's
// re-dock setting the panel state back to 'docked' and never telling the pop-out window to
// close, so the operator ended up with two waterfalls — a "close" that changed state the
// window did not read. Every close path here has to reach `closePanelWindow`, the command
// that fix added, and this file checks each of them by name rather than trusting that they
// all funnel through one handler: the ✕ and Esc are two code paths, and the whole shape of
// #263 is one of them being wired and the other not.
//
// jsdom cannot lay this out, and nothing below asks it to. What it CAN do is mount the real
// component and count what is on screen, which is what the details, the stepping and the
// close assertions are.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor } from '@testing-library/react'
import * as api from '../api'
import { EN } from '../i18n/en'
import { SstvViewer, SSTV_VIEWER_PANEL, SSTV_VIEWER_PATH_KEY, setViewerPicture } from './SstvViewer'

vi.mock('../api', () => ({
  getSstvState: vi.fn(),
  closePanelWindow: vi.fn(async () => {}),
  revealSstvGallery: vi.fn(async () => {}),
  savePngToDownloads: vi.fn(async () => '/home/op/Downloads/pic.png'),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => {
    try {
      return await action()
    } catch {
      return null
    }
  }),
}))

const getSstvState = api.getSstvState as unknown as ReturnType<typeof vi.fn>
const closePanelWindow = api.closePanelWindow as unknown as ReturnType<typeof vi.fn>

/** Oldest first, exactly as the engine hands the gallery over — the viewer reverses it, so a
 *  test that built the list newest-first would silently agree with a viewer that did not. */
const GALLERY = [
  {
    path: '/g/img-001-martin1.png',
    mode: 'Martin 1',
    finishedUtc: '2026-09-15T12:00:00Z',
    freqMhz: 14.23,
    lines: 256,
    fskId: null,
  },
  {
    path: '/g/img-002-scottie1.png',
    mode: 'Scottie 1',
    finishedUtc: '2026-09-15T12:10:00Z',
    freqMhz: 14.233,
    lines: 256,
    fskId: 'W1AW',
  },
  {
    path: '/g/img-003-pd120.png',
    mode: 'PD-120',
    finishedUtc: '2026-09-15T12:20:00Z',
    freqMhz: 14.236,
    lines: 496,
    fskId: null,
  },
]

const state = (gallery = GALLERY) => ({
  armed: true,
  gallery,
  health: {},
  sending: false,
  txMode: null,
  txProgress: 0,
  txElapsedSecs: 0,
  txTotalSecs: 0,
})

beforeEach(() => {
  localStorage.clear()
  getSstvState.mockReset().mockResolvedValue(state())
  closePanelWindow.mockReset().mockResolvedValue(undefined)
})
afterEach(cleanup)

describe('the SSTV picture viewer', () => {
  it('shows the picture the main window pointed it at, with its own details', async () => {
    setViewerPicture('/g/img-002-scottie1.png')
    render(<SstvViewer />)
    // Mode, the decoded FSK callsign, and the when/where line — the four facts the
    // operator asked to be able to see without leaving Nexus.
    expect(await screen.findByText('Scottie 1')).toBeTruthy()
    expect(screen.getByText('W1AW')).toBeTruthy()
    expect(screen.getByText(/2026-09-15 12:10Z/)).toBeTruthy()
    expect(screen.getByText(/14\.233/)).toBeTruthy()
  })

  it('falls back to the newest picture when it was opened without one', async () => {
    render(<SstvViewer />)
    // Newest is img-003, not the first row the engine handed over.
    expect(await screen.findByText('PD-120')).toBeTruthy()
  })

  it('arrow keys step through the gallery, newest first, and wrap', async () => {
    setViewerPicture('/g/img-003-pd120.png')
    render(<SstvViewer />)
    await screen.findByText('PD-120')

    fireEvent.keyDown(window, { key: 'ArrowRight' })
    expect(await screen.findByText('Scottie 1')).toBeTruthy()
    fireEvent.keyDown(window, { key: 'ArrowRight' })
    expect(await screen.findByText('Martin 1')).toBeTruthy()
    // Wraps rather than dead-ending, so a held arrow never strands the operator.
    fireEvent.keyDown(window, { key: 'ArrowRight' })
    expect(await screen.findByText('PD-120')).toBeTruthy()
    fireEvent.keyDown(window, { key: 'ArrowLeft' })
    expect(await screen.findByText('Martin 1')).toBeTruthy()

    // The step is written back, so the main window and this one agree about which picture
    // is open and re-opening the viewer lands on the one last looked at.
    expect(localStorage.getItem(SSTV_VIEWER_PATH_KEY)).toBe('/g/img-001-martin1.png')
  })

  it('⭐ Esc CLOSES THE WINDOW — not a flag, the window (#263)', async () => {
    render(<SstvViewer />)
    await screen.findByText('PD-120')
    fireEvent.keyDown(window, { key: 'Escape' })
    expect(closePanelWindow).toHaveBeenCalledWith(SSTV_VIEWER_PANEL)
  })

  it('⭐ the Close button closes the window too — a SECOND path, checked separately', async () => {
    // #263 was one close path wired and another not. Funnelling both through one handler
    // is the right implementation; proving it is a different act from assuming it.
    render(<SstvViewer />)
    fireEvent.click(await screen.findByRole('button', { name: EN['sstv.viewer.close.label'] }))
    expect(closePanelWindow).toHaveBeenCalledWith(SSTV_VIEWER_PANEL)
  })

  it('follows the main window when another thumbnail is clicked while it is open', async () => {
    setViewerPicture('/g/img-003-pd120.png')
    render(<SstvViewer />)
    await screen.findByText('PD-120')
    // A torn-off window is a separate JS realm; the main window re-points it by writing the
    // key, which arrives here as a `storage` event. This is what makes ONE viewer window
    // the right answer instead of one window per picture.
    fireEvent(
      window,
      Object.assign(new Event('storage'), {
        key: SSTV_VIEWER_PATH_KEY,
        newValue: '/g/img-001-martin1.png',
      }),
    )
    expect(await screen.findByText('Martin 1')).toBeTruthy()
  })

  it('saves a copy of the picture it is showing', async () => {
    const save = api.savePngToDownloads as unknown as ReturnType<typeof vi.fn>
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => ({ arrayBuffer: async () => new Uint8Array([1, 2, 3]).buffer })),
    )
    // The asset protocol is a Tauri shim; without it there is no URL to fetch, which is the
    // real desktop shape — stub the converter the component asks for.
    ;(window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {
      convertFileSrc: (p: string) => `asset://${p}`,
    }
    setViewerPicture('/g/img-002-scottie1.png')
    render(<SstvViewer />)
    fireEvent.click(await screen.findByRole('button', { name: EN['sstv.viewer.save.label'] }))
    // Named from the picture's own caption, never from the station's file path.
    await waitFor(() =>
      expect(save).toHaveBeenCalledWith(
        expect.stringContaining('nexus-sstv-2026-09-15T12-10-00Z-Scottie-1'),
        expect.any(String),
      ),
    )
    vi.unstubAllGlobals()
  })

  it('says so rather than showing nothing when the gallery is empty', async () => {
    getSstvState.mockResolvedValue(state([]))
    render(<SstvViewer />)
    expect(await screen.findByText(EN['sstv.viewer.empty'])).toBeTruthy()
  })

  it('steps to a neighbour rather than going blank when the picture is deleted under it', async () => {
    setViewerPicture('/g/img-002-scottie1.png')
    render(<SstvViewer />)
    await screen.findByText('Scottie 1')
    // The main window deleted it; the next poll no longer lists it. The viewer must not be
    // left pointing at a file that is gone — it shows the newest instead.
    getSstvState.mockResolvedValue(state([GALLERY[0], GALLERY[2]]))
    expect(await screen.findByText('PD-120', {}, { timeout: 6000 })).toBeTruthy()
  })
})
