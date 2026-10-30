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

  it('control: another chip joining the lane leaves the alarm chip in place', () => {
    render(<StatusLane />)
    act(() => setStatus('audio', { tier: 'critical', message: 'RADIO ENGINE STOPPED' }))
    const first = chip('RADIO ENGINE STOPPED')
    act(() => setStatus('scope', { tier: 'warning', message: 'NO RF SCOPE' }))
    expect(chip('RADIO ENGINE STOPPED')).toBe(first)
  })
})
