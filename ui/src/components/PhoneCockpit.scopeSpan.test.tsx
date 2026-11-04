// @vitest-environment jsdom
//
// The Phone audio scope's span chip is remembered per window. Full (0–5000 Hz) is the default
// (operator decision, 2026-11-03: audio views open wide, the filter drawn as its passband box);
// Auto (the rig's filter width) stays a chip, and a stored Auto is kept. A stored value no chip
// names reads as Full. Under the chips: the radio's filter named where it is what limits the
// picture, and the Icom native-scope hint, both the CW scope's own.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent, act } from '@testing-library/react'
import { PhoneCockpit } from './PhoneCockpit'
import type { AppSnapshot } from '../types'

const { settingsNow } = vi.hoisted(() => ({
  // The settings the cockpit reads on open: the active radio's model, connection, baud and CI-V switch.
  settingsNow: { value: {} as Record<string, unknown> },
}))
vi.mock('../api', () => ({
  // The Phone cockpit reads the FM repeater shift from Settings — it is the only surface
  // that carries it, and the transmit contract will not state a frequency without it. The
  // scope's Icom hint reads the radio's link from it too.
  getSettings: vi.fn(async () => ({ ...settingsNow.value })),
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


beforeEach(() => {
  localStorage.clear()
  settingsNow.value = {}
})
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

const mount = (radio: Record<string, unknown>, onOpenSettings?: (where: string) => void) =>
  render(<PhoneCockpit snap={snapWith(radio)} theme="dark" onOpenSettings={onOpenSettings} />)
/** Mount, then let the settings read land. */
async function mountRead(radio: Record<string, unknown>, onOpenSettings?: (where: string) => void) {
  const r = mount(radio, onOpenSettings)
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

/** The audio window's width the scope was last handed (carrier-centred: 0..width). */
const width = () => scopeProps[scopeProps.length - 1].viewHiHz as number
const chip = (name: string) => screen.getByRole('button', { name })

describe('the Phone audio scope span', () => {
  it('opens on Full: 0–5000 Hz, with the radio’s filter drawn on it as its passband', () => {
    mount({ filterWidthHz: 2400 })
    expect(screen.getByRole('group', { name: 'Bandscope span' })).toBeTruthy()
    expect(chip('Full').getAttribute('aria-pressed')).toBe('true')
    expect(chip('Auto').getAttribute('aria-pressed')).toBe('false')
    expect(width()).toBe(5000)
    expect(scopeProps[scopeProps.length - 1].passbandHz).toBe(2400)
    expect(chip('Full').getAttribute('title')).toContain('5 kHz')
    expect(localStorage.getItem('nexus.phone.scope.span'), 'opening writes nothing').toBeNull()
  })

  it('a stored Auto pick is kept: exactly the rig filter width, for framing it', () => {
    localStorage.setItem('nexus.phone.scope.span', 'auto')
    mount({ filterWidthHz: 2400 })
    expect(chip('Auto').getAttribute('aria-pressed')).toBe('true')
    expect(width()).toBe(2400)
    cleanup()
    // An FM filter wider than the row is clamped to the row, not to the old 4 kHz.
    localStorage.setItem('nexus.phone.scope.span', 'auto')
    mount({ filterWidthHz: 12000 })
    expect(width()).toBe(5000)
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

  it('a stored value no chip names opens on Full', () => {
    localStorage.setItem('nexus.phone.scope.span', 'wide')
    mount({ filterWidthHz: 2400 })
    expect(chip('Full').getAttribute('aria-pressed')).toBe('true')
    expect(width()).toBe(5000)
  })
})

describe('the Phone audio scope says when the radio’s filter is what limits it', () => {
  const note = () => screen.queryByText(/widen it on the radio to see more/)

  it('names the filter in Full and Voice, where the window is wider than it', () => {
    mount({ filterWidthHz: 2400 })
    expect(note()?.textContent).toBe("Showing your radio's 2400 Hz filter — widen it on the radio to see more.")
    fireEvent.click(chip('Voice'))
    expect(note()).not.toBeNull()
  })

  it('never in Auto, which frames the filter by design', () => {
    mount({ filterWidthHz: 2400 })
    fireEvent.click(chip('Auto'))
    expect(note()).toBeNull()
    // Positive control on the same render.
    fireEvent.click(chip('Full'))
    expect(note()).not.toBeNull()
  })

  it('says nothing when the filter covers the window, is unknown, or the feed is RF', () => {
    mount({ filterWidthHz: 2400 })
    fireEvent.click(chip('1.5k'))
    expect(note()).toBeNull()
    cleanup()
    // The 1.5k pick is kept; back to a window that never picked one.
    localStorage.clear()
    mount({ filterWidthHz: null })
    expect(note()).toBeNull()
    cleanup()
    mount({ filterWidthHz: 2400 })
    expect(note()).not.toBeNull()
    const onFeed = scopeProps[scopeProps.length - 1].onFeed as (s: string, lo: number, hi: number) => void
    act(() => onFeed('civ', 14_000_000, 14_100_000))
    expect(note()).toBeNull()
  })
})

describe('the Phone scope points an Icom at its own wide scope', () => {
  const icomOff = { rigModel: 3081, rigConn: 'serial', icomNativeCat: false, baud: 115200 }
  const hint = () => screen.queryByText(/can show its own wide scope/)
  const baudHint = () => screen.queryByText(/needs 115,200 baud/)

  it('on an IC-9700 over Hamlib: says so, and the link opens the switch', async () => {
    settingsNow.value = icomOff
    const open = vi.fn()
    await mountRead({ filterWidthHz: 2400 }, open)
    expect(hint()).not.toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /Native Icom CI-V/ }))
    expect(open).toHaveBeenCalledWith('rig-advanced')
  })

  it('with the switch on below 115,200 baud: says the baud is why', async () => {
    settingsNow.value = { ...icomOff, icomNativeCat: true, baud: 57600 }
    const open = vi.fn()
    await mountRead({ filterWidthHz: 2400 }, open)
    expect(hint()).toBeNull()
    expect(baudHint()).not.toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /Rig & CAT/ }))
    expect(open).toHaveBeenCalledWith('rig-control')
  })

  it('says nothing where no setting would bring the scope, and goes once it streams', async () => {
    for (const s of [
      { ...icomOff, icomNativeCat: true },
      { ...icomOff, rigConn: 'icomlan' },
      // A Yaesu: Nexus cannot read its scope over CAT at all.
      { ...icomOff, rigModel: 1042 },
      { ...icomOff, nativeScope: 'audio' },
    ]) {
      settingsNow.value = s
      await mountRead({ filterWidthHz: 2400 }, vi.fn())
      expect(hint(), JSON.stringify(s)).toBeNull()
      expect(baudHint(), JSON.stringify(s)).toBeNull()
      cleanup()
    }
    settingsNow.value = icomOff
    await mountRead({ filterWidthHz: 2400 }, vi.fn())
    expect(hint()).not.toBeNull()
    const onFeed = scopeProps[scopeProps.length - 1].onFeed as (s: string, lo: number, hi: number) => void
    act(() => onFeed('civ', 14_000_000, 14_100_000))
    expect(hint()).toBeNull()
  })
})
