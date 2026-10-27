// @vitest-environment jsdom
//
// REPAIR CLOCK: THE ONE ADMINISTRATOR PROMPT NEXUS RAISES, AND ONLY ON THIS PRESS.
//
// The clock check used to run its repair by itself, so operators met a Windows administrator
// prompt nobody had asked for (operator, 2026-10-06: "Only when you press Repair"). The backend
// now only diagnoses and offers; this button is how an offered repair runs. These tests pin when
// it is drawn, that a press is one call, and that every outcome is said in words.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, act } from '@testing-library/react'
import { TopBar } from './TopBar'
import type { RadioStatus } from '../types'
import { EN } from '../i18n'
import { repairClock } from '../api'
import { pushToast } from '../toast'

vi.mock('../api', () => ({
  appVersion: vi.fn(() => Promise.resolve('0.0.0')),
  repairClock: vi.fn(),
}))
// A computer driving a station (client mode): off unless a case turns it on.
const driving = vi.hoisted(() => ({ on: false }))
vi.mock('../features/durableStore', async (actual) => ({
  ...(await actual<typeof import('../features/durableStore')>()),
  drivesAnotherStation: () => driving.on,
}))
vi.mock('../toast', async (original) => ({
  ...(await original<typeof import('../toast')>()),
  pushToast: vi.fn(),
}))

const repair = vi.mocked(repairClock)
const toast = vi.mocked(pushToast)

beforeEach(() => {
  repair.mockReset()
  toast.mockReset()
})
afterEach(cleanup)

const base = {
  dialMhz: 14.074,
  band: '20m',
  sideband: 'USB',
  rigMode: 'USB',
  rigConfirmed: true,
  nextSlotMs: 0,
  txEven: true,
  txCycleAuto: true,
  txEnabled: false,
  txAllowed: true,
  transmitting: false,
  tuning: false,
  qsoRecording: false,
  catOk: true,
  dtSec: 0,
  timeSyncOk: true,
  clockOffsetMs: 420,
  clockAgeSecs: 60,
  clockServers: 3,
  clockOwnerNote: 'the Windows Time service is not running — nothing is keeping this clock right',
  clockRepairAvailable: true,
} as unknown as RadioStatus

function renderBar(radio: Record<string, unknown> = {}) {
  const noop = () => {}
  return render(
    <TopBar
      mycall="KD9TAW"
      mygrid="EN52xa"
      radio={{ ...base, ...radio } as RadioStatus}
      link={{ tier: 'FT8' } as never}
      bandPlan={[]}
      onSetFrequency={noop}
      onSetTxEnabled={noop}
      onSetTune={noop}
      onHaltTx={noop}
      onSetTxEven={noop}
      onSetTxCycleAuto={noop}
      onSetHoldTxFreq={noop}
      tier="FT8"
      onTierChange={noop}
      onOpenGuide={noop}
      field={false}
      onFieldChange={noop}
    />,
  )
}

const button = () => screen.queryByRole('button', { name: EN['topbar.clock.repair.label'] })

describe('when Repair clock is drawn', () => {
  it('under the chip when a repair is on offer, with the line saying Windows will ask', () => {
    renderBar()
    const b = button()
    expect(b).not.toBeNull()
    expect(b!.closest('.clock-repair')!.previousElementSibling!.className).toContain('timesync')
    expect(screen.getByText(EN['topbar.clock.repair.note'])).not.toBeNull()
    // The line is the button's description, and the tooltip names what the check found.
    expect(b!.getAttribute('aria-describedby')).toBe(screen.getByText(EN['topbar.clock.repair.note']).id)
    expect(b!.getAttribute('title')).toContain('is not running')
  })

  it('not at all when nothing is on offer (every machine that needs nothing, and Linux and macOS)', () => {
    renderBar({ clockRepairAvailable: false })
    expect(button()).toBeNull()
    expect(screen.queryByText(EN['topbar.clock.repair.note'])).toBeNull()
    cleanup()
    renderBar({ clockRepairAvailable: undefined })
    expect(button()).toBeNull()
  })
})

describe('a press', () => {
  it('runs the repair once, reads busy while Windows asks, and says it worked', async () => {
    let answer: (took: boolean) => void = () => {}
    repair.mockReturnValue(new Promise<boolean>((r) => (answer = r)))
    renderBar()
    fireEvent.click(button()!)
    expect(repair).toHaveBeenCalledTimes(1)
    const busy = screen.getByRole('button', { name: EN['topbar.clock.repair.busy'] })
    expect((busy as HTMLButtonElement).disabled).toBe(true)
    fireEvent.click(busy)
    expect(repair, 'a second click while Windows asks is no second prompt').toHaveBeenCalledTimes(1)
    await act(async () => answer(true))
    expect(toast).toHaveBeenCalledWith(EN['topbar.clock.repair.done'], 'success')
    expect((button() as HTMLButtonElement).disabled).toBe(false)
  })

  it('says so when the repair did not take', async () => {
    repair.mockResolvedValue(false)
    renderBar()
    await act(async () => fireEvent.click(button()!))
    expect(toast).toHaveBeenCalledWith(EN['topbar.clock.repair.failed'], 'error')
  })

  it.each([
    ['onAir', 'topbar.clock.repair.onAir'],
    ['midMessage', 'topbar.clock.repair.midMessage'],
    ['repairRunning', 'topbar.clock.repair.running'],
    ['nothingToRepair', 'topbar.clock.repair.nothing'],
    ['clock repair task failed: cancelled', 'topbar.clock.repair.failed'],
  ] as const)('says which refusal it was: %s', async (code, key) => {
    repair.mockRejectedValue(code)
    renderBar()
    await act(async () => fireEvent.click(button()!))
    expect(toast).toHaveBeenCalledWith(EN[key], 'error')
  })
})

// On a computer driving a station the repair is the station's: client mode refuses it in words
// (`client.refused.shackOnly`), and those are what the toast says. It used to say the repair did
// not take, which read as a declined administrator prompt.
describe('on a computer driving a station', () => {
  afterEach(() => {
    driving.on = false
  })
  it('says it is only at the station, in the words the refusal came with', async () => {
    driving.on = true
    repair.mockRejectedValue(EN['client.refused.shackOnly'])
    renderBar()
    await act(async () => fireEvent.click(button()!))
    expect(toast).toHaveBeenCalledWith(EN['client.refused.shackOnly'], 'error')
  })
})

describe('on the air', () => {
  it.each([
    ['an over of our own', { transmitting: true }],
    ['a rig keyed at the radio', { rigKeyed: true }],
    ['a tune carrier', { tuning: true }],
  ])('is disabled during %s, and says why', (_what, keyed) => {
    renderBar(keyed)
    const b = button() as HTMLButtonElement
    expect(b.disabled).toBe(true)
    expect(b.title).toContain(EN['topbar.clock.repair.onAir'])
    fireEvent.click(b)
    expect(repair).not.toHaveBeenCalled()
  })

  it('and enabled with the transmitter idle (the control)', () => {
    renderBar()
    expect((button() as HTMLButtonElement).disabled).toBe(false)
  })
})
