// @vitest-environment jsdom
//
// The Phone audio scope's span chip is remembered per window. Auto (the rig's filter width) stays
// the default, and a stored value no chip names reads as Auto.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'

vi.mock('../api', () => ({
  // The Phone cockpit reads the FM repeater shift from Settings — it is the only surface
  // that carries it, and the transmit contract will not state a frequency without it.
  getSettings: vi.fn(async () => ({})),
  setPtt: vi.fn(async () => ({})),
  setTxEnabled: vi.fn(async () => ({})),
  setRfPower: vi.fn(async () => {}),
  setMicGain: vi.fn(async () => {}),
  setNrLevel: vi.fn(async () => {}),
  // The two #95 additions.
  setCompLevel: vi.fn(async () => {}),
  setNotchFreq: vi.fn(async () => {}),
  setAgc: vi.fn(async () => ({})),
  setScopeSpan: vi.fn(async () => ({})),
  setScopeRef: vi.fn(async () => {}),
  setFlexPanSpan: vi.fn(async () => ({})),
  setFlexPanRef: vi.fn(async () => ({})),
  startQsoRecording: vi.fn(async () => ({})),
  stopQsoRecording: vi.fn(async () => ({})),
  setTune: vi.fn(async () => ({})),
  haltTx: vi.fn(async () => ({})),
  setFrequency: vi.fn(async () => ({})),
  setSplit: vi.fn(async () => ({})),
  setRigFunc: vi.fn(async () => ({})),
  setSidebandOverride: vi.fn(async () => ({})),
  setFilterWidth: vi.fn(async () => ({})),
  openPanelWindow: vi.fn(async () => {}),
}))
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (action: () => Promise<unknown>) => action()),
}))
// The scope, band strip, keyer and log form are not what this file is about; stubbing them
// keeps the failure surface to the DSP pane. CockpitHeader is stubbed for the same reason —
// unlike the stop-line sweep, nothing here asserts on a header control.
vi.mock('./CockpitHeader', () => ({ CockpitHeader: () => <header className="cockpit-header" /> }))
const { scopeProps } = vi.hoisted(() => ({ scopeProps: [] as Record<string, unknown>[] }))
vi.mock('./PhoneScope', () => ({
  PhoneScope: (p: Record<string, unknown>) => {
    scopeProps.push(p)
    return <div data-testid="scope-stub" />
  },
}))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./VoiceKeyer', () => ({ VoiceKeyer: () => <div data-testid="vk-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))


beforeEach(() => localStorage.clear())
afterEach(() => {
  cleanup()
  scopeProps.length = 0
})

/** A Phone-cockpit snapshot with the rig reporting exactly the DSP surface given. */
function snapWith(radio: Record<string, unknown>): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    radio: {
      dialMhz: 14.2,
      band: '20m',
      sideband: 'USB',
      // ⚠️ catOk, EXPLICITLY. Every case here is about what the RADIO reports; a dead CAT
      // link is a different question with its own answer (the pane banner), and it would
      // otherwise swallow every one of these by explaining them all at once.
      catOk: true,
      transmitting: false,
      txEnabled: false,
      txAllowed: true,
      slot: 0,
      nextSlotMs: 0,
      ...radio,
    },
    link: {},
    qso: {},
    stations: [],
    conversations: [],
  } as unknown as AppSnapshot
}

const mount = (radio: Record<string, unknown>) =>
  render(<PhoneCockpit snap={snapWith(radio)} theme="dark" />)

/** The audio window's width the scope was last handed (carrier-centred: 0..width). */
const width = () => scopeProps[scopeProps.length - 1].viewHiHz as number
const chip = (name: string) => screen.getByRole('button', { name })

describe('the Phone audio scope span', () => {
  it('opens on Auto: the rig filter width', () => {
    mount({ filterWidthHz: 2400 })
    expect(screen.getByRole('group', { name: 'Bandscope span' })).toBeTruthy()
    expect(chip('Auto').getAttribute('aria-pressed')).toBe('true')
    expect(width()).toBe(2400)
  })

  it('keeps the chip picked for the next open', () => {
    mount({ filterWidthHz: 2400 })
    fireEvent.click(chip('Voice'))
    expect(width()).toBe(2700)
    expect(localStorage.getItem('nexus.phone.scope.span')).toBe('voice')
    cleanup()
    mount({ filterWidthHz: 2400 })
    expect(chip('Voice').getAttribute('aria-pressed')).toBe('true')
    expect(chip('Auto').getAttribute('aria-pressed')).toBe('false')
    expect(width()).toBe(2700)
  })

  it('a stored value no chip names opens on Auto', () => {
    localStorage.setItem('nexus.phone.scope.span', 'wide')
    mount({ filterWidthHz: 2400 })
    expect(chip('Auto').getAttribute('aria-pressed')).toBe('true')
    expect(width()).toBe(2400)
  })
})
