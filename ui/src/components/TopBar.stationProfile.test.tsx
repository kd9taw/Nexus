// @vitest-environment jsdom
//
// The "My station" picker in the top bar: absent for a station that never made a profile (the bar
// it always had), one select once there are two (Home and one more), each option naming the call
// it transmits under, and switched only at the station — from a computer driving it the picker is
// shown but cannot change the call the station transmits under.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { TopBar } from './TopBar'
import type { RadioStatus, StationProfileChoice } from '../types'
import { setRemoteStation } from '../features/durableStore'

vi.setConfig({ testTimeout: 15_000 })

afterEach(() => {
  cleanup()
  setRemoteStation(false)
})

const radio = {
  dialMhz: 14.074, band: '20m', sideband: 'USB', rigMode: 'USB', rigConfirmed: true, nextSlotMs: 0,
  txEven: true, txCycleAuto: true, txEnabled: false, txAllowed: true, transmitting: false,
  tuning: false, qsoRecording: false, catOk: true, dtSec: 0, clockOffsetMs: 0,
} as unknown as RadioStatus

const both: StationProfileChoice[] = [
  { id: 'home', name: '', call: 'KD9TAW' },
  { id: 'p1', name: 'Portable', call: 'KD9TAW/P' },
]

function renderBar(over: Record<string, unknown> = {}) {
  const noop = () => {}
  return render(
    <TopBar mycall="KD9TAW" mygrid="EN52" radio={radio} link={{ tier: 'FT8' } as never} bandPlan={[]}
      onSetFrequency={noop} onSetTxEnabled={noop} onSetTune={noop} onHaltTx={noop} onSetTxEven={noop}
      onSetTxCycleAuto={noop} onSetHoldTxFreq={noop} tier="FT8" onTierChange={noop} onOpenGuide={noop}
      {...over} />,
  )
}

describe('the My station picker', () => {
  it('is not there for a station with no profiles', () => {
    renderBar({ stationProfiles: undefined })
    expect(screen.queryByLabelText('Station profile')).toBeNull()
    cleanup()
    renderBar({ stationProfiles: both.slice(0, 1), activeStationProfile: 'home' })
    expect(screen.queryByLabelText('Station profile')).toBeNull()
  })

  it('offers each profile with its call, and switches on a pick', () => {
    const onSwitch = vi.fn()
    renderBar({ stationProfiles: both, activeStationProfile: 'home', onSwitchStationProfile: onSwitch })
    const picker = screen.getByLabelText('Station profile') as HTMLSelectElement
    expect(picker.value).toBe('home')
    expect([...picker.options].map((o) => o.textContent)).toEqual(['Home · KD9TAW', 'Portable · KD9TAW/P'])
    expect(picker.disabled).toBe(false)
    fireEvent.change(picker, { target: { value: 'p1' } })
    expect(onSwitch).toHaveBeenCalledWith('p1')
  })

  it('waits, saying why, while a run, a contact or an over is under way', () => {
    const onSwitch = vi.fn()
    renderBar({
      stationProfiles: both, activeStationProfile: 'home', stationProfileLocked: true, onSwitchStationProfile: onSwitch,
    })
    const picker = screen.getByLabelText('Station profile') as HTMLSelectElement
    expect(picker.disabled).toBe(true)
    expect(picker.title).toBe('Waits for the run or contact to end: stop transmitting and press S&P, then switch.')
    cleanup()
    renderBar({
      stationProfiles: both, activeStationProfile: 'home', stationProfileLocked: false, onSwitchStationProfile: onSwitch,
    })
    expect((screen.getByLabelText('Station profile') as HTMLSelectElement).disabled, 'CONTROL: idle').toBe(false)
  })

  it('is switched at the station only', () => {
    setRemoteStation(true)
    renderBar({ stationProfiles: both, activeStationProfile: 'home', onSwitchStationProfile: vi.fn() })
    const picker = screen.getByLabelText('Station profile') as HTMLSelectElement
    expect(picker.disabled).toBe(true)
    expect(picker.title).toBe('Switched at the station only.')
  })
})
