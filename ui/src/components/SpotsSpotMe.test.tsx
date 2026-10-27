// @vitest-environment jsdom
//
// The Spots board's Spot me (every cockpit that shows the board): ONE cluster spot per operator
// press and never on a timer, a comment that opens on "CQ <mode>" and keeps the last one typed for
// each mode, the station's one cooldown, and with Contest mode on the contest's self-spot rule exactly
// as on the contest screen. The station holds the cooldown and the rules (contest_spot.rs); this pins
// the board's half, through the board itself, by value.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, fireEvent, act, cleanup } from '@testing-library/react'

const clusterSelfSpot = vi.fn()
const contestSelfSpot = vi.fn()
const getPskState = vi.fn()
vi.mock('../api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../api')>()),
  clusterSelfSpot: (c: string, ok: boolean) => clusterSelfSpot(c, ok),
  contestSelfSpot: (c: string, ok: boolean) => contestSelfSpot(c, ok),
  getPskState: () => getPskState(),
}))
const pushToast = vi.fn()
vi.mock('../toast', async (importOriginal) => ({
  ...(await importOriginal<typeof import('../toast')>()),
  pushToast: (...a: unknown[]) => pushToast(...a),
}))

import { SpotsPanel } from './SpotsPanel'

type Station = { section?: string; keyboardMode?: string; sideband?: string; tier?: string }

const board = (spotMe?: Station) => (
  <SpotsPanel spots={[]} bandPlan={[]} selectedCall={null} onSelect={() => {}} onWork={() => {}} pane={{ scope: 'phone' }} spotMe={spotMe} />
)

const posted = (comment: string) => ({ freqKhz: 14025.5, call: 'KD9TAW', comment, cooldownSecs: 60 })

beforeEach(() => {
  vi.useFakeTimers()
  localStorage.clear()
  clusterSelfSpot.mockReset()
  contestSelfSpot.mockReset()
  getPskState.mockReset()
  getPskState.mockResolvedValue({ mode: 'psk31' })
  pushToast.mockReset()
})
afterEach(() => {
  cleanup()
  vi.useRealTimers()
})

const comment = () => screen.getByRole('textbox', { name: 'Spot comment' }) as HTMLInputElement
const button = (name = 'Spot me') => screen.getByRole('button', { name }) as HTMLButtonElement
const settle = async () => {
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
}

describe('Spot me on the Spots board', () => {
  it('opens the comment on CQ and the mode the station is on', async () => {
    const { rerender } = render(board({ section: 'cw', sideband: 'CW' }))
    expect(comment().value).toBe('CQ CW')
    const seen: string[] = []
    for (const station of [
      { section: 'phone', sideband: 'USB' },
      { section: 'phone', sideband: 'LSB' },
      { section: 'phone', sideband: 'FM' },
      { section: 'phone', sideband: 'AM' },
      { section: 'rtty', sideband: 'RTTY' },
      { section: 'digital', sideband: 'PKTUSB', tier: 'FT8' },
    ]) {
      rerender(board(station))
      seen.push(comment().value)
    }
    expect(seen).toEqual(['CQ SSB', 'CQ SSB', 'CQ FM', 'CQ AM', 'CQ RTTY', 'CQ FT8'])
    // The keyboard modes name the PSK mode the PSK screen is on.
    getPskState.mockResolvedValue({ mode: 'qpsk31' })
    rerender(board({ section: 'keyboard', sideband: 'PKTUSB' }))
    await settle()
    expect(comment().value).toBe('CQ QPSK31')
    // PSK and JTTY are one section: the station's own word for the mode on the air wins.
    rerender(board({ section: 'keyboard', sideband: 'PKTUSB', keyboardMode: 'JTTY' }))
    await settle()
    expect(comment().value).toBe('CQ JTTY')
  })

  it('keeps the last comment typed for each mode, and an emptied box gives the default back', () => {
    const { rerender, unmount } = render(board({ section: 'cw' }))
    fireEvent.change(comment(), { target: { value: 'CQ CW QRS 15' } })
    rerender(board({ section: 'phone', sideband: 'USB' }))
    expect(comment().value).toBe('CQ SSB')
    fireEvent.change(comment(), { target: { value: 'CQ SSB net' } })
    rerender(board({ section: 'cw' }))
    expect(comment().value).toBe('CQ CW QRS 15')
    unmount()
    // A board opened later (another cockpit, a restart) reads the same.
    const again = render(board({ section: 'phone', sideband: 'LSB' }))
    expect(comment().value).toBe('CQ SSB net')
    fireEvent.change(comment(), { target: { value: '' } })
    again.rerender(board({ section: 'cw' }))
    again.rerender(board({ section: 'phone', sideband: 'LSB' }))
    expect(comment().value).toBe('CQ SSB')
  })

  it('reads a stored record that is not one as none, and a comment too long for the cluster cut to fit', () => {
    for (const junk of ['not json', '[1,2]', 'null', '{"CW": 7}', '{"CW": "   "}']) {
      localStorage.setItem('nexus.spotMe.comments', junk)
      render(board({ section: 'cw' }))
      expect(comment().value).toBe('CQ CW')
      cleanup()
    }
    localStorage.setItem('nexus.spotMe.comments', JSON.stringify({ CW: 'CQ CW '.repeat(10) }))
    render(board({ section: 'cw' }))
    expect(comment().value).toBe('CQ CW CQ CW CQ CW CQ CW CQ CW ')
  })

  it('posts nothing by itself: on screen ten minutes, across a mode change and typing, no spot', async () => {
    const { rerender } = render(board({ section: 'cw' }))
    fireEvent.change(comment(), { target: { value: 'CQ CW 599' } })
    rerender(board({ section: 'phone', sideband: 'USB' }))
    await act(async () => {
      vi.advanceTimersByTime(10 * 60_000)
    })
    expect(clusterSelfSpot).not.toHaveBeenCalled()
    expect(contestSelfSpot).not.toHaveBeenCalled()
  })

  it('one press posts one spot with the comment and says the line that went', async () => {
    clusterSelfSpot.mockImplementation((c: string) => Promise.resolve(posted(c)))
    render(board({ section: 'cw' }))
    fireEvent.change(comment(), { target: { value: 'CQ CW QRS' } })
    fireEvent.click(button())
    await settle()
    expect(clusterSelfSpot.mock.calls).toEqual([['CQ CW QRS', false]])
    expect(contestSelfSpot).not.toHaveBeenCalled()
    expect(pushToast.mock.calls).toEqual([['Spotted to the DX cluster: DX 14025.5 KD9TAW CQ CW QRS', 'success']])
  })

  it('a double-click is one spot, and the button waits out the cooldown before the next', async () => {
    let resolve: (v: ReturnType<typeof posted>) => void = () => {}
    clusterSelfSpot.mockImplementation(() => new Promise((r) => (resolve = r)))
    render(board({ section: 'cw' }))
    fireEvent.click(button())
    fireEvent.click(button())
    expect(clusterSelfSpot).toHaveBeenCalledTimes(1)
    resolve(posted('CQ CW'))
    await settle()
    expect(button('Wait 60 s').disabled).toBe(true)
    fireEvent.click(button('Wait 60 s'))
    await act(async () => {
      vi.advanceTimersByTime(30_000)
    })
    expect(button('Wait 30 s').disabled).toBe(true)
    await act(async () => {
      vi.advanceTimersByTime(30_000)
    })
    expect(button().disabled).toBe(false)
    expect(clusterSelfSpot).toHaveBeenCalledTimes(1)
  })

  it.each([
    ['spotNoCall', 'Not spotted: set your callsign in Settings first.'],
    ['spotNoCluster', 'Not spotted: no DX cluster is connected. Set a cluster in Settings ▸ Spot Sources.'],
    // The contest screen's spot went a moment ago: the station's one cooldown holds this button too.
    ['spotCooldown:42', 'Not spotted again yet: one spot a minute. 42 s to go.'],
    ['spotOffBand', 'Not spotted: the dial is outside the amateur bands.'],
  ])('a refusal (%s) is said in words, and the button is free again at once', async (token, words) => {
    clusterSelfSpot.mockRejectedValue(token)
    render(board({ section: 'cw' }))
    fireEvent.click(button())
    await settle()
    expect(pushToast.mock.calls).toEqual([[words, 'error']])
    expect(button().disabled).toBe(false)
  })

  describe('with Contest mode on, the contest self-spot rule as on the contest screen', () => {
    const warning = 'This contest may not allow self-spotting. Press Spot anyway within 10 s to post.'
    const unconfirmed = (c: string, ok: boolean) => (ok ? Promise.resolve(posted(c)) : Promise.reject('spotSelfUnconfirmed'))

    it('the first press posts nothing and warns; a second press inside the window posts', async () => {
      clusterSelfSpot.mockImplementation(unconfirmed)
      render(board({ section: 'cw' }))
      fireEvent.click(button())
      await settle()
      expect(clusterSelfSpot.mock.calls).toEqual([['CQ CW', false]])
      expect(pushToast.mock.calls).toEqual([[warning, 'info']])
      await act(async () => {
        vi.advanceTimersByTime(9_000)
      })
      fireEvent.click(button('Spot anyway'))
      await settle()
      expect(clusterSelfSpot.mock.calls).toEqual([
        ['CQ CW', false],
        ['CQ CW', true],
      ])
      expect(pushToast).toHaveBeenLastCalledWith('Spotted to the DX cluster: DX 14025.5 KD9TAW CQ CW', 'success')
      expect(button('Wait 60 s').disabled).toBe(true)
    })

    it('after the window a press is a first press again, and the warning never posts by itself', async () => {
      clusterSelfSpot.mockImplementation(unconfirmed)
      render(board({ section: 'phone', sideband: 'USB' }))
      fireEvent.click(button())
      await settle()
      await act(async () => {
        vi.advanceTimersByTime(60_000)
      })
      expect(clusterSelfSpot.mock.calls).toEqual([['CQ SSB', false]])
      fireEvent.click(button())
      await settle()
      expect(clusterSelfSpot.mock.calls).toEqual([
        ['CQ SSB', false],
        ['CQ SSB', false],
      ])
    })

    it.each([
      ['spotNotAllowed', "Not spotted: this contest's rules allow no spotting."],
      ['spotRfOnly', 'Not spotted: this contest allows spotting only over amateur radio while it runs.'],
    ])('where the contest refuses (%s), every press is refused in words', async (token, words) => {
      clusterSelfSpot.mockRejectedValue(token)
      render(board({ section: 'cw' }))
      fireEvent.click(button())
      await settle()
      fireEvent.click(button())
      await settle()
      expect(clusterSelfSpot.mock.calls).toEqual([
        ['CQ CW', false],
        ['CQ CW', false],
      ])
      expect(pushToast.mock.calls).toEqual([
        [words, 'error'],
        [words, 'error'],
      ])
    })
  })

  it('is not on a board its host gave no station', () => {
    render(board(undefined))
    expect(screen.queryByRole('button', { name: 'Spot me' })).toBeNull()
    // Positive control: the same board with control and a station shows it.
    cleanup()
    render(board({ section: 'cw' }))
    expect(button().textContent).toBe('Spot me')
  })
})
