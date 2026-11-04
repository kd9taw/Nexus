// @vitest-environment jsdom
//
// THE PHONE SCOPE VIEWS on a radio whose own scope Nexus draws: an RF / Audio chip in the scope head
// (no Both: Phone keeps one view), RF by default as before, kept per window and validated on load;
// the radio span chips the one control of the radio's span; and the slow-scope hint.
import { describe, it, expect, afterEach, beforeEach, vi } from 'vitest'
import { render, screen, cleanup, fireEvent, act, within } from '@testing-library/react'
import * as api from '../api'
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
    return <div data-testid="scope-stub" data-feed={(p.feed as string | undefined) ?? 'scope'} />
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


const feeds = () => [...document.querySelectorAll('[data-testid="scope-stub"]')].map((e) => e.getAttribute('data-feed'))
const last = () => scopeProps[scopeProps.length - 1]
type Report = (s: string, lo: number, hi: number) => void
const radioScopeSeen = (source = 'civ') => act(() => (last().onFeed as Report)(source, 14_150_000, 14_250_000))
const group = () => screen.queryByRole('group', { name: 'What the scope shows' })
const viewChip = (name: string) => within(group()!).getByRole('button', { name })

describe('the Phone scope on a radio with its own scope', () => {
  beforeEach(() => {
    vi.mocked(api.setScopeSpan).mockClear()
    vi.mocked(api.setFlexPanSpan).mockClear()
  })

  it('keeps the radio scope by default, and offers RF / Audio once that scope has been seen', async () => {
    await mountRead({})
    expect(group()).toBeNull()
    radioScopeSeen()
    expect(feeds()).toEqual(['scope'])
    expect([...within(group()!).getAllByRole('button')].map((b) => b.textContent)).toEqual(['RF', 'Audio'])
    expect(viewChip('RF').getAttribute('aria-pressed')).toBe('true')
    // Phone's default span is still Band.
    expect(within(screen.getByRole('group', { name: 'Panadapter zoom' })).getByRole('button', { name: 'Band' }).getAttribute('aria-pressed')).toBe('true')
  })

  it('the chip round trip: Audio reads the audio only, RF the rig scope, each kept for this window', async () => {
    await mountRead({})
    radioScopeSeen()
    fireEvent.click(viewChip('Audio'))
    expect(feeds()).toEqual(['audio'])
    expect(localStorage.getItem('nexus.phone.scopeView')).toBe('audio')
    expect(screen.getByRole('group', { name: 'Bandscope span' })).toBeTruthy()
    fireEvent.click(viewChip('RF'))
    expect(feeds()).toEqual(['scope'])
    expect(localStorage.getItem('nexus.phone.scopeView')).toBe('rf')
  })

  it('validates the stored view on load: Both (a CW word) or junk opens on RF', async () => {
    for (const bad of ['both', 'bogus', '']) {
      localStorage.setItem('nexus.phone.scopeView', bad)
      await mountRead({})
      radioScopeSeen()
      expect(feeds(), `stored ${JSON.stringify(bad)}`).toEqual(['scope'])
      expect(viewChip('RF').getAttribute('aria-pressed')).toBe('true')
      cleanup()
    }
    localStorage.setItem('nexus.phone.scopeView', 'audio')
    await mountRead({})
    radioScopeSeen()
    expect(feeds()).toEqual(['audio'])
  })

  it('ONE span owner: no rig-span row beside the span chips, and the Ref level stays', async () => {
    await mountRead({})
    radioScopeSeen()
    expect(screen.queryAllByTitle(/scope span to|panadapter bandwidth to/)).toEqual([])
    expect(screen.getByRole('slider', { name: 'Scope reference level (dB)' })).toBeTruthy()
    const z = within(screen.getByRole('group', { name: 'Panadapter zoom' }))
    expect(z.getByRole('button', { name: '±100k' })).toBeTruthy()
    expect(z.getByRole('button', { name: '±50k' })).toBeTruthy()
  })

  it('the slow-scope hint on an Icom only, and never a command to the radio', async () => {
    await mountRead({})
    radioScopeSeen()
    const hint = () => screen.queryByText("Set the radio's scope SPEED to FAST for a smoother scope")
    act(() => (last().onSweepRate as (r: number | null) => void)(3))
    expect(hint()).not.toBeNull()
    act(() => (last().onSweepRate as (r: number | null) => void)(9))
    expect(hint()).toBeNull()
    expect(vi.mocked(api.setScopeSpan).mock.calls.length + vi.mocked(api.setFlexPanSpan).mock.calls.length).toBe(0)
  })
})
