// @vitest-environment jsdom
//
// THE POPPED-OUT OPERATE WINDOW SAYS WHY. Its one handler for every FT button (Enable TX, a call,
// Tx1-5, free text, resend, the run mode, Tune, Halt) dropped each failure, so a press refused by
// the station, or by client mode on a computer driving one, did nothing and said nothing; the main
// window's same buttons have always toasted. And its Log took the snapshot and dropped the engine's
// verdict: "nothing to log" was never said, as the main window says it.
import { describe, it, expect, vi, beforeAll, afterEach } from 'vitest'
import { render, cleanup, act, screen, fireEvent, waitFor } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import * as api from './api'
import { pushToast } from './toast'

const SNAP = vi.hoisted(() => ({
  radio: { band: '20m', dialMhz: 14.074, slot: 0 },
  link: { tier: 'FT8' },
  conversations: [],
}))

vi.mock('./api', async () => {
  const actual = await vi.importActual<Record<string, unknown>>('./api')
  const out: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    out[k] = typeof actual[k] === 'function' ? vi.fn().mockResolvedValue(null) : actual[k]
  }
  out.subscribeSnapshot = vi.fn((cb: (s: unknown) => void) => {
    cb(SNAP)
    return () => {}
  })
  out.getBandPlan = vi.fn().mockResolvedValue([])
  out.getNeedAlerts = vi.fn().mockResolvedValue([])
  return out
})
vi.mock('./toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn((f: () => Promise<unknown>) => f().catch(() => null)),
  subscribeToasts: vi.fn(() => () => {}),
  subscribePopups: vi.fn(() => () => {}),
  setPopupNotifications: vi.fn(),
  setPopupMinSeconds: vi.fn(),
  dismissToast: vi.fn(),
}))
// The cockpit itself is not under test: a button per handler this window gives it.
vi.mock('./components/OperateCockpit', () => ({
  OperateCockpit: (p: { onSetTxEnabled: (on: boolean) => void; onLog: () => void }) => (
    <div>
      <button type="button" onClick={() => p.onSetTxEnabled(true)}>Enable TX</button>
      <button type="button" onClick={() => p.onLog()}>Log</button>
    </div>
  ),
}))
vi.mock('./components/StationList', () => ({ StationList: () => null }))

beforeAll(() => {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  vi.clearAllMocks()
})

describe('the popped-out Operate window', () => {
  it('shows a refused Enable TX in the words it came with', async () => {
    vi.mocked(api.setTxEnabled).mockRejectedValueOnce('Not connected to the station.')
    render(<DetachedPanel panel="operate" />)
    await act(async () => {})
    fireEvent.click(screen.getByRole('button', { name: 'Enable TX' }))
    await waitFor(() => expect(pushToast).toHaveBeenCalledWith('Not connected to the station.', 'error'))
  })

  it('says when there was nothing to log, as the main window does', async () => {
    vi.mocked(api.logCurrentQso).mockResolvedValueOnce({ logged: false, snapshot: SNAP as never })
    render(<DetachedPanel panel="operate" />)
    await act(async () => {})
    fireEvent.click(screen.getByRole('button', { name: 'Log' }))
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith(
        'Nothing to log — the QSO already closed or no report was exchanged',
        'info',
        4000,
      ),
    )
  })

  it('a logged QSO says so', async () => {
    vi.mocked(api.logCurrentQso).mockResolvedValueOnce({ logged: true, snapshot: SNAP as never })
    render(<DetachedPanel panel="operate" />)
    await act(async () => {})
    fireEvent.click(screen.getByRole('button', { name: 'Log' }))
    await waitFor(() => expect(pushToast).toHaveBeenCalledWith('Logged QSO', 'success', 2500))
  })

  it('names the station it logged, which may not be the one shown', async () => {
    vi.mocked(api.logCurrentQso).mockResolvedValueOnce({ logged: true, call: '7P8RU', snapshot: SNAP as never })
    render(<DetachedPanel panel="operate" />)
    await act(async () => {})
    fireEvent.click(screen.getByRole('button', { name: 'Log' }))
    await waitFor(() => expect(pushToast).toHaveBeenCalledWith('Logged QSO with 7P8RU', 'success', 2500))
  })

  it('says the contact already waits in the popup when nothing new was held, not that it logged', async () => {
    vi.mocked(api.logCurrentQso).mockResolvedValueOnce({
      logged: true,
      call: '7P8RU',
      alreadyWaiting: true,
      snapshot: SNAP as never,
    })
    render(<DetachedPanel panel="operate" />)
    await act(async () => {})
    fireEvent.click(screen.getByRole('button', { name: 'Log' }))
    await waitFor(() =>
      expect(pushToast).toHaveBeenCalledWith('7P8RU is already waiting in the log popup', 'info', 4000),
    )
    expect(pushToast).not.toHaveBeenCalledWith('Logged QSO with 7P8RU', 'success', 2500)
  })
})
