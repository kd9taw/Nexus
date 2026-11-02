// @vitest-environment jsdom
//
// The Phone FreeDV pane, by what the operator reads and what a click does. Receive only: the one
// action that changes anything is turning the heard-call listener on, and a heard call's click
// hands the call to the log strip and nothing else.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { cleanup, render, screen, fireEvent, waitFor } from '@testing-library/react'
import { getSettings, setSettings } from '../api'
import { FreedvPane } from './FreedvPane'
import type { FreedvState } from '../types'
import { dismissToast, subscribeToasts, type Toast } from '../toast'

vi.mock('../api', () => ({
  getSettings: vi.fn(() => Promise.resolve({ mycall: 'K2DEF', freedvHeard: false })),
  setSettings: vi.fn(() => Promise.resolve({})),
}))

const NOW = 1_791_939_723

function state(over: Partial<FreedvState> = {}): FreedvState {
  return {
    logPort: 2239,
    logError: null,
    aliveUnix: null,
    lastContact: null,
    lastContactUnix: null,
    notice: null,
    heardPort: null,
    heardError: null,
    heard: [],
    ...over,
  }
}

afterEach(cleanup)

beforeEach(() => {
  vi.mocked(getSettings).mockClear()
  vi.mocked(setSettings).mockClear()
})

describe('FreedvPane', () => {
  it('says where to point FreeDV until it is heard from, then that it is connected', () => {
    const { rerender } = render(<FreedvPane freedv={state()} control onPickCall={() => {}} nowUnix={NOW} />)
    expect(screen.getByRole('status').textContent).toBe('Waiting for FreeDV: point its QSO Logging at 127.0.0.1:2239.')
    rerender(<FreedvPane freedv={state({ aliveUnix: NOW - 30 })} control onPickCall={() => {}} nowUnix={NOW} />)
    expect(screen.getByRole('status').textContent).toBe('FreeDV connected')
    // A heartbeat long gone is not "connected".
    rerender(<FreedvPane freedv={state({ aliveUnix: NOW - 600 })} control onPickCall={() => {}} nowUnix={NOW} />)
    expect(screen.getByRole('status').textContent).toMatch(/^Waiting for FreeDV/)
  })

  it('shows a refused bind, the last contact and a refusal as the station wrote them', () => {
    render(
      <FreedvPane
        freedv={state({
          logPort: null,
          logError: 'FreeDV contacts cannot listen on 127.0.0.1:2239: another program holds that port.',
          lastContact: 'W5FDV · 14.236 MHz',
          notice: 'FreeDV logged N0FDV as W1AW, not imported: this station is K2DEF',
        })}
        control
        onPickCall={() => {}}
        nowUnix={NOW}
      />,
    )
    expect(screen.getByRole('status').textContent).toContain('another program holds that port')
    expect(screen.getByText('Last contact: W5FDV · 14.236 MHz')).toBeTruthy()
    expect(screen.getAllByRole('alert')[0].textContent).toBe(
      'FreeDV logged N0FDV as W1AW, not imported: this station is K2DEF',
    )
  })

  it('a heard call fills the log strip, and nothing else', () => {
    const pick = vi.fn()
    render(
      <FreedvPane
        freedv={state({
          heardPort: 7177,
          heard: [
            { call: 'W5FDV', mode: 'RADEV1', snr: 7, frequencyHz: 14_236_000, heardUnix: NOW },
            { call: 'VK5DGR', mode: '', snr: null, frequencyHz: null, heardUnix: NOW - 5 },
          ],
        })}
        control
        onPickCall={pick}
        nowUnix={NOW}
      />,
    )
    const list = screen.getByRole('list', { name: 'Calls FreeDV heard' })
    expect(list.textContent).toBe('W5FDVRADEV1 · 7 dB · 14.236 MHzVK5DGR')
    fireEvent.click(screen.getByRole('button', { name: 'W5FDV' }))
    expect(pick).toHaveBeenCalledTimes(1)
    expect(pick).toHaveBeenCalledWith('W5FDV')
    expect(setSettings).not.toHaveBeenCalled()
  })

  it('turns the heard-call listener on with the settings as stored, that one field changed', async () => {
    render(<FreedvPane freedv={state()} control onPickCall={() => {}} nowUnix={NOW} />)
    fireEvent.click(screen.getByRole('button', { name: 'Listen for them' }))
    await waitFor(() => expect(setSettings).toHaveBeenCalledTimes(1))
    expect(vi.mocked(setSettings).mock.calls[0][0]).toEqual({ mycall: 'K2DEF', freedvHeard: true })
  })

  it('a listener switch that cannot be saved says so, and writes only its own field', async () => {
    let toasts: Toast[] = []
    const off = subscribeToasts((now) => {
      toasts = now
    })
    vi.mocked(setSettings).mockRejectedValueOnce(new Error('disk full'))
    render(<FreedvPane freedv={state()} control onPickCall={() => {}} nowUnix={NOW} />)
    fireEvent.click(screen.getByRole('button', { name: 'Listen for them' }))
    await waitFor(() => expect(toasts.map((x) => x.kind)).toContain('error'))
    const said = toasts.find((x) => x.kind === 'error')!
    expect(said.message).toMatch(/^Could not save settings\./)
    expect(said.message).toContain('disk full')
    // Its one field over the settings read at save time; nothing else from this window.
    expect(vi.mocked(setSettings).mock.calls[0][0]).toEqual({ mycall: 'K2DEF', freedvHeard: true })
    toasts.forEach((x) => dismissToast(x.id))
    off()
  })

  it('offers no settings change on a screen without control, and says when nothing listens', () => {
    const { rerender } = render(<FreedvPane freedv={state()} control={false} onPickCall={() => {}} nowUnix={NOW} />)
    expect(screen.queryByRole('button', { name: 'Listen for them' })).toBeNull()
    rerender(<FreedvPane freedv={undefined} control onPickCall={() => {}} nowUnix={NOW} />)
    expect(screen.getByRole('status').textContent).toMatch(/^Nexus is not listening for FreeDV/)
  })
})
