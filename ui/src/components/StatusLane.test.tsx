// @vitest-environment jsdom
//
// A NEW ALARM PULSES AGAIN. The critical chip's pulse runs a few times and then holds steady
// (styles-idle-repaint.test.ts), and a CSS animation only starts over on a new element. The audio
// line puts every kind of audio problem under the one status id, so a chip keyed by id alone would
// stay mounted when "RADIO ENGINE STOPPED" gave way to a different critical alarm, and the new
// alarm would arrive without a pulse. Element identity is what decides whether the animation runs
// again, so that is what this pins: a new message is a new chip; the same alarm keeps its chip.
import { describe, it, expect, afterEach } from 'vitest'
import { render, cleanup, screen, act } from '@testing-library/react'
import { StatusLane } from './StatusLane'
import { setStatus } from '../status'

const chip = (words: string) => screen.getByText(words).closest('.status-chip')

afterEach(() => {
  act(() => {
    setStatus('audio', null)
    setStatus('scope', null)
  })
  cleanup()
})

describe('the status lane remounts a chip only for a new alarm', () => {
  it('a different critical alarm under the same id is a new chip, so its pulse starts over', () => {
    render(<StatusLane />)
    act(() => setStatus('audio', { tier: 'critical', message: 'RADIO ENGINE STOPPED', detail: 'unplugged' }))
    const first = chip('RADIO ENGINE STOPPED')
    expect(first?.className).toContain('tier-critical')
    act(() => setStatus('audio', { tier: 'critical', message: 'SOUND CARD FAILED', detail: 'unplugged' }))
    const second = chip('SOUND CARD FAILED')
    expect(second).not.toBeNull()
    expect(second).not.toBe(first)
    expect(first?.isConnected).toBe(false)
  })

  it('control: the same alarm with a reworded tooltip keeps its chip (no second round of pulses)', () => {
    render(<StatusLane />)
    act(() => setStatus('audio', { tier: 'critical', message: 'RADIO ENGINE STOPPED', detail: 'unplugged' }))
    const first = chip('RADIO ENGINE STOPPED')
    act(() => setStatus('audio', { tier: 'critical', message: 'RADIO ENGINE STOPPED', detail: 'unplugged again' }))
    expect(chip('RADIO ENGINE STOPPED')).toBe(first)
  })

  it('control: the same alarm re-sent on every snapshot and re-rendered keeps its chip (no flicker)', () => {
    const { rerender } = render(<StatusLane />)
    act(() => setStatus('audio', { tier: 'critical', message: 'RADIO ENGINE STOPPED', detail: 'unplugged' }))
    const first = chip('RADIO ENGINE STOPPED')
    for (let i = 0; i < 5; i++) {
      act(() => setStatus('audio', { tier: 'critical', message: 'RADIO ENGINE STOPPED', detail: 'unplugged' }))
      rerender(<StatusLane />)
    }
    expect(chip('RADIO ENGINE STOPPED')).toBe(first)
    expect(first?.isConnected).toBe(true)
  })

  it('control: another chip joining the lane leaves the alarm chip in place', () => {
    render(<StatusLane />)
    act(() => setStatus('audio', { tier: 'critical', message: 'RADIO ENGINE STOPPED' }))
    const first = chip('RADIO ENGINE STOPPED')
    act(() => setStatus('scope', { tier: 'warning', message: 'NO RF SCOPE' }))
    expect(chip('RADIO ENGINE STOPPED')).toBe(first)
  })
})

// OFF THE OPERATING SCREENS (no alarm cell beside Stop TX) the lane is the only place a critical item shows:
// it keeps the flag (`.status-lane-keep`), comes first, ahead of REC, and is announced as an alert. On the
// operating screens the alarm cell carries it, and the lane neither floors nor alerts.
/** The lane's first drawn item, after the gap it gives back (`.status-lane-gap`). */
const firstItem = (lane: Element) => [...lane.children].find((e) => !e.classList.contains('status-lane-gap'))

describe('a critical item with no alarm cell on the screen', () => {
  afterEach(() => {
    for (const id of ['k-crit', 'k-warn']) setStatus(id, null)
  })
  const rec = <button type="button" className="topbar-rec">REC</button>
  it('keepCritical: kept, first, an alert; a warning is not', () => {
    render(<StatusLane keepCritical lead={rec} />)
    act(() => {
      setStatus('k-warn', { tier: 'warning', message: 'NO RF SCOPE' })
      setStatus('k-crit', { tier: 'critical', message: 'PTT OFF FAILED — TX STOPPED' })
    })
    const lane = document.querySelector('.status-lane')!
    expect(lane.classList.contains('status-lane-keep')).toBe(true)
    expect(firstItem(lane)?.textContent).toContain('PTT OFF FAILED')
    expect(screen.getByRole('alert').textContent).toContain('PTT OFF FAILED')
    expect(screen.getAllByRole('alert')).toHaveLength(1)
  })
  it('with the alarm cell (no keepCritical): no floor and no alert, REC first', () => {
    render(<StatusLane lead={rec} />)
    act(() => setStatus('k-crit', { tier: 'critical', message: 'PTT OFF FAILED — TX STOPPED' }))
    const lane = document.querySelector('.status-lane')!
    expect(lane.classList.contains('status-lane-keep')).toBe(false)
    expect(screen.queryByRole('alert')).toBeNull()
    expect(firstItem(lane)?.classList.contains('topbar-rec')).toBe(true)
  })
  // The floor is held whether or not a critical item is up: one ARRIVING must not wrap the bar and move its
  // Stop TX (the Logbook's, the only one on such a screen).
  it('keepCritical with nothing critical: the floor is held all the same', () => {
    render(<StatusLane keepCritical lead={rec} />)
    act(() => setStatus('k-warn', { tier: 'warning', message: 'NO RF SCOPE' }))
    expect(document.querySelector('.status-lane')!.classList.contains('status-lane-keep')).toBe(true)
  })
})

// ALWAYS DRAWN (2026-11-08): the lane stands where the bar's free room is, so a box that came and went with a
// message moved every group after it, the Logbook's Stop TX among them (937 → 450 px at 1920x1080).
describe('the lane box with nothing in it', () => {
  it('is drawn, empty but for the gap it gives back, on both kinds of screen', () => {
    for (const keepCritical of [false, true]) {
      render(<StatusLane keepCritical={keepCritical} />)
      const lane = document.querySelector('.status-lane')
      expect(lane, `keepCritical=${keepCritical}: no lane box`).not.toBeNull()
      expect([...lane!.children].map((e) => e.className)).toEqual(['status-lane-gap'])
      cleanup()
    }
  })
})

