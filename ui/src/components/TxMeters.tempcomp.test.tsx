// @vitest-environment jsdom
//
// FINALS TEMPERATURE AND THE COMP ROW'S CONTROLS (operator request; #247).
//
// The temperature rows appear only where a reading arrives — the radio's own PA in °C (converted
// to °F under the Imperial units setting), a radio's meter-only TEMP bar as a share of its scale,
// an amplifier's — and are live between overs. The COMP row sets what it shows: drag or arrow
// keys set the processor level, PROC switches the processor, and the transmit-bandwidth chip
// cycles the radio's own steps; each only where the host passes the verb AND the radio read the
// value back. jsdom never lays out, so the drag is driven against a stubbed track box.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, cleanup, fireEvent, screen } from '@testing-library/react'
import { TxMeters } from './TxMeters'
import type { RadioStatus } from '../types'
import { UNITS_KEY } from '../units'

beforeEach(() => localStorage.setItem(UNITS_KEY, 'metric'))
afterEach(() => {
  cleanup()
  localStorage.clear()
})

const radio = (over: Record<string, unknown> = {}) => ({ ...over }) as unknown as RadioStatus
const temps = () =>
  [...document.querySelectorAll('.ph-txmeter.temp')].map((r) => [
    r.querySelector('.ph-txmeter-label')?.textContent,
    r.querySelector('.ph-txmeter-value')?.textContent,
  ])
const kpa = (temp: number) => ({ family: 'kpa', model: '', linked: true, reason: '', temp, tempCelsius: true, alarm: '' })
const spe = (temp: number) => ({ family: 'spe', model: '13K', linked: true, reason: '', temp, tempCelsius: false, alarm: '' })

describe('the finals temperature', () => {
  it('is hidden on a radio that reports none and has no amplifier set up', () => {
    render(<TxMeters radio={radio()} pinned />)
    expect(temps()).toEqual([])
  })

  // STOP TX STAYS PUT (2026-11-08): the amplifier's row is drawn whenever an amplifier is set up, with "—"
  // while it has no reading. Drawn only while linked with a temperature, it came with the first answer and
  // went with a dropped link, and the dock under Stop TX changed height (Phone and CW at 900x600, real
  // Chrome). Every state of the amplifier draws the same rows, under the same labels.
  it('an amplifier draws the same rows in every state: not answered yet, linked, lost, linked with no reading', () => {
    const notYet = { family: 'kpa', model: '', linked: false, reason: '', tempCelsius: false, alarm: '' }
    const lost = { ...kpa(40), linked: false, reason: 'noAnswer', temp: null }
    const states = [notYet, kpa(52), lost, { ...kpa(52), temp: null }]
    const shapes = states.map((amp) => {
      const r = render(<TxMeters radio={radio({ paTempC: 38, amp })} pinned />)
      const rows = temps()
      r.unmount()
      return rows
    })
    expect(shapes[1]).toEqual([['RIG', '38°C'], ['AMP', '52°C']])
    for (const s of shapes) expect(s.map(([label]) => label), 'the same rows, the same labels').toEqual(['RIG', 'AMP'])
    expect(shapes[0][1][1], 'no reading: a dash, never an old number').toBe('—')
    expect(shapes[2][1][1]).toBe('—')
    // An amplifier with no radio reading beside it: its row alone, in every state.
    render(<TxMeters radio={radio({ amp: notYet })} pinned />)
    expect(temps()).toEqual([['AMP', '—']])
  })

  it('reads the radio in °C, before any over, and in °F under Imperial', () => {
    const r = render(<TxMeters radio={radio({ paTempC: 45 })} pinned />)
    expect(temps()).toEqual([['TEMP', '45°C']])
    r.unmount()
    localStorage.setItem(UNITS_KEY, 'imperial')
    render(<TxMeters radio={radio({ paTempC: 45 })} pinned />)
    expect(temps()).toEqual([['TEMP', '113°F']])
  })

  it('shows a meter-only radio as its own meter, never as degrees', () => {
    render(<TxMeters radio={radio({ paTempMeter: 0.4 })} pinned />)
    expect(temps()).toEqual([['TEMP', '40%']])
  })

  it('labels the radio RIG beside an amplifier, converts a Celsius amplifier and leaves SPE bare', () => {
    const r = render(<TxMeters radio={radio({ paTempC: 38, amp: kpa(52) })} pinned />)
    expect(temps()).toEqual([
      ['RIG', '38°C'],
      ['AMP', '52°C'],
    ])
    r.unmount()
    render(<TxMeters radio={radio({ amp: spe(41) })} pinned />)
    // SPE's wire does not say °C or °F: no letter, no conversion.
    expect(temps()).toEqual([['AMP', '41°']])
  })

  it('shows in the Operate cell before the first over too', () => {
    render(<TxMeters radio={radio({ paTempC: 50 })} inline />)
    expect(document.querySelector('.ph-txmeters-hint')).not.toBeNull()
    expect(temps()).toEqual([['TEMP', '50°C']])
  })
})

describe('the COMP row sets the processor', () => {
  const comp = () => document.querySelector('.ph-txmeter.settable')!
  const slider = () => screen.queryByRole('slider', { name: 'Speech processor level' })

  it('is a meter only, with no host verbs or no read-back', () => {
    render(<TxMeters radio={radio({ compLevel: 0.5, comp: true, txBwSteps: ['WIDE', 'MID', 'NAR'], txBw: 'MID' })} pinned />)
    expect(slider()).toBeNull()
    expect(document.querySelector('.ph-txmeter-btn')).toBeNull()
    cleanup()
    render(<TxMeters radio={radio()} pinned controls={{ onCompLevel: vi.fn(), onProc: vi.fn(), onTxBw: vi.fn() }} />)
    expect(slider()).toBeNull()
    expect(document.querySelector('.ph-txmeter-btn')).toBeNull()
  })

  it('arrow keys step the level by 5 from the last step, and Home/End go to the ends', () => {
    const onCompLevel = vi.fn()
    render(<TxMeters radio={radio({ compLevel: 0.5 })} pinned controls={{ onCompLevel }} />)
    const s = slider()!
    expect(s.getAttribute('aria-valuenow')).toBe('50')
    fireEvent.keyDown(s, { key: 'ArrowRight' })
    expect(onCompLevel).toHaveBeenLastCalledWith(0.55)
    // Each press steps from the one before, not from the radio's last report.
    fireEvent.keyDown(s, { key: 'ArrowUp' })
    expect(onCompLevel).toHaveBeenLastCalledWith(0.6)
    fireEvent.keyDown(s, { key: 'ArrowDown' })
    expect(onCompLevel).toHaveBeenLastCalledWith(0.55)
    fireEvent.keyDown(s, { key: 'End' })
    expect(onCompLevel).toHaveBeenLastCalledWith(1)
    fireEvent.keyDown(s, { key: 'Home' })
    expect(onCompLevel).toHaveBeenLastCalledWith(0)
  })

  it('a drag sets the level where the pointer is on the bar, and the marker follows', () => {
    const onCompLevel = vi.fn()
    render(<TxMeters radio={radio({ compLevel: 0.1, rigKeyed: true, txCompDb: 6 })} pinned controls={{ onCompLevel }} />)
    const s = slider()!
    s.getBoundingClientRect = () => ({ left: 100, width: 200, top: 0, height: 10, right: 300, bottom: 10, x: 100, y: 0, toJSON: () => ({}) })
    fireEvent.pointerDown(s, { clientX: 250, pointerId: 1 })
    expect(onCompLevel).toHaveBeenLastCalledWith(0.75)
    fireEvent.pointerMove(s, { clientX: 150, pointerId: 1 })
    expect(onCompLevel).toHaveBeenLastCalledWith(0.25)
    expect((comp().querySelector('.ph-txmeter-mark') as HTMLElement).style.left).toBe('25%')
    // The bar is still the compression meter while the level moves.
    expect(comp().querySelector('.ph-txmeter-value')?.textContent).toBe('6 dB')
    fireEvent.pointerUp(s, { pointerId: 1 })
    const calls = onCompLevel.mock.calls.length
    fireEvent.pointerMove(s, { clientX: 290, pointerId: 1 })
    expect(onCompLevel.mock.calls.length, 'no drag after release').toBe(calls)
  })

  it('PROC shows the processor state and switches it, where the radio loop offers it', () => {
    const onProc = vi.fn()
    render(<TxMeters radio={radio({ comp: true, txProcSwitch: true })} pinned controls={{ onProc }} />)
    const b = document.querySelector('[data-ctl="proc"]') as HTMLButtonElement
    expect(b.getAttribute('aria-pressed')).toBe('true')
    fireEvent.click(b)
    expect(onProc).toHaveBeenCalledWith(false)
  })

  it('PROC is not offered on a link whose switch write is not the maker\'s own (a Yaesu through Hamlib)', () => {
    render(<TxMeters radio={radio({ comp: true })} pinned controls={{ onProc: vi.fn() }} />)
    expect(document.querySelector('[data-ctl="proc"]')).toBeNull()
  })

  it('after a drag released mid-over, the marker keeps the new level until the radio reports one', () => {
    const onCompLevel = vi.fn()
    const keyed = { compLevel: 0.1, rigKeyed: true, txCompDb: 6 }
    const r = render(<TxMeters radio={radio(keyed)} pinned controls={{ onCompLevel }} />)
    const s = slider()!
    s.getBoundingClientRect = () => ({ left: 100, width: 200, top: 0, height: 10, right: 300, bottom: 10, x: 100, y: 0, toJSON: () => ({}) })
    fireEvent.pointerDown(s, { clientX: 250, pointerId: 1 })
    fireEvent.pointerUp(s, { pointerId: 1 })
    // The radio is not read back mid-over: its reported level is still the old one.
    r.rerender(<TxMeters radio={radio({ ...keyed, txCompDb: 7 })} pinned controls={{ onCompLevel }} />)
    expect(slider()!.getAttribute('aria-valuenow')).toBe('75')
    expect((comp().querySelector('.ph-txmeter-mark') as HTMLElement).style.left).toBe('75%')
    // The read-back between overs is the truth again, whatever it says.
    r.rerender(<TxMeters radio={radio({ compLevel: 0.7 })} pinned controls={{ onCompLevel }} />)
    expect(slider()!.getAttribute('aria-valuenow')).toBe('70')
  })

  it('an arrow-key step mid-over is held on the marker the same way', () => {
    const r = render(<TxMeters radio={radio({ compLevel: 0.5, rigKeyed: true })} pinned controls={{ onCompLevel: vi.fn() }} />)
    fireEvent.keyDown(slider()!, { key: 'ArrowRight' })
    r.rerender(<TxMeters radio={radio({ compLevel: 0.5, rigKeyed: true, txCompDb: 3 })} pinned controls={{ onCompLevel: vi.fn() }} />)
    expect(slider()!.getAttribute('aria-valuenow')).toBe('55')
  })

  it('the bandwidth chip cycles the radio\'s own steps, wrapping, and is held while on the air', () => {
    const onTxBw = vi.fn()
    const steps = ['WIDE', 'MID', 'NAR']
    const r = render(<TxMeters radio={radio({ txBwSteps: steps, txBw: 'MID' })} pinned controls={{ onTxBw }} />)
    const chip = () => document.querySelector('[data-ctl="txbw"]') as HTMLButtonElement
    expect(chip().textContent).toBe('MID')
    fireEvent.click(chip())
    expect(onTxBw).toHaveBeenLastCalledWith('NAR')
    r.rerender(<TxMeters radio={radio({ txBwSteps: steps, txBw: 'NAR' })} pinned controls={{ onTxBw }} />)
    fireEvent.click(chip())
    expect(onTxBw).toHaveBeenLastCalledWith('WIDE')
    r.rerender(<TxMeters radio={radio({ txBwSteps: steps, txBw: 'NAR', rigKeyed: true })} pinned controls={{ onTxBw }} />)
    expect(chip().disabled).toBe(true)
  })

  it('the bandwidth chip is hidden until the radio has read a step back', () => {
    render(<TxMeters radio={radio({ txBwSteps: ['WIDE', 'MID', 'NAR'] })} pinned controls={{ onTxBw: vi.fn() }} />)
    expect(document.querySelector('[data-ctl="txbw"]')).toBeNull()
  })
})
