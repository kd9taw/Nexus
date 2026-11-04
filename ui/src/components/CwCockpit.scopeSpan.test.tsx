// @vitest-environment jsdom
//
// The CW audio scope's span chips: Filter (the rig's filter round the pitch, the zero-beating view),
// Std (200–3000 Hz, the default) and Full (200–5000 Hz), the waterfalls' own views. The pick is kept
// per window, Filter included; a window that never pressed one, or holds a value no chip names,
// opens on Std (operator decision, 2026-11-03: audio views open wide, the filter drawn as edges).
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, act } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import { cwScopeWindow } from '../waterfall'
import type { AppSnapshot } from '../types'
import type { ReactNode } from 'react'

const decodeState = {
  text: '',
  wpm: 22,
  sent: [] as string[],
  keyerError: null as string | null,
  candidates: [] as { call: string; best: boolean }[],
  state: 'listening',
  headline: '',
  prompt: '',
  recommended: null as string | null,
  workedCall: null as string | null,
  rst: null as string | null,
  name: null as string | null,
}

const { setCwKeyer, settingsNow } = vi.hoisted(() => ({
  setCwKeyer: vi.fn(async (..._a: unknown[]) => null),
  // The settings the cockpit reads on open: the active radio's model, connection, baud and CI-V switch.
  settingsNow: { value: {} as Record<string, unknown> },
}))

vi.mock('../api', async (importOriginal) => {
  // ⭐ DERIVED FROM THE REAL MODULE, not a hand-kept list. A hand-kept mock omits any export
  // added after it was written, and a component that calls one THROWS ON MOUNT — so the suite
  // goes red at a seam nothing in the diff explains, and the tempting fix is to make the test
  // pass rather than ask why. That cost five files one evening when a single API call was added
  // to the CW cockpit, this one among them.
  //
  // Every function the module exports is auto-stubbed here; the entries below override only the
  // ones this file's assertions actually depend on, so their shapes are unchanged.
  const actual = await importOriginal<Record<string, unknown>>()
  const auto: Record<string, unknown> = {}
  for (const k of Object.keys(actual)) {
    auto[k] = typeof actual[k] === 'function' ? vi.fn(async () => ({})) : actual[k]
  }
  return {
    ...auto,
    // Hand-kept mock: an export CwCockpit calls but this list omits makes it THROW ON MOUNT,
    // which reads as a behaviour regression rather than the stale mock it actually is.
    getCatCwUnprovenRigModels: vi.fn(async () => []),
    getSettings: vi.fn(async () => ({ macros: { cwProfiles: [], activeCwProfile: 0 }, ...settingsNow.value })),
    setSettings: vi.fn(async () => ({})),
    sendCw: vi.fn(async () => {}),
    setCwKeyer,
    setCwWpm: vi.fn(async () => {}),
    stopCw: vi.fn(async () => {}),
    cwDecode: vi.fn(async () => decodeState),
    cwClear: vi.fn(async () => {}),
    setAiCw: vi.fn(async () => {}),
    selectPeer: vi.fn(async () => null),
    previewCw: vi.fn(async (t: string) => t),
    pointRotatorAtCall: vi.fn(async () => 0),
    setRigFunc: vi.fn(async () => ({})),
    setFilterWidth: vi.fn(async () => ({})),
    setNrLevel: vi.fn(async () => {}),
    setScopeSpan: vi.fn(async () => ({})),
    setScopeRef: vi.fn(async () => {}),
    setFlexPanSpan: vi.fn(async () => ({})),
    setFlexPanRef: vi.fn(async () => ({})),
    openPanelWindow: vi.fn(async () => {}),
    setTune: vi.fn(async () => ({})),
    setFrequency: vi.fn(async () => ({})),
    haltTx: vi.fn(async () => ({})),
  }
})

// The REAL CockpitHeader: the pitch box is one of its children. Its band and tuning controls are stubbed.
vi.mock('./BandPicker', () => ({ BandPicker: () => null }))
vi.mock('./TuningStrip', () => ({ TuningStrip: () => null }))
// The scope records the window the cockpit hands it; the scope's own click mapping is
// PhoneScope.cwSpan.test.tsx's.
const { scopeProps } = vi.hoisted(() => ({ scopeProps: [] as Record<string, unknown>[] }))
vi.mock('./PhoneScope', () => ({
  PhoneScope: (p: Record<string, unknown>) => {
    scopeProps.push(p)
    // The scope's own toolbar strip, holding whatever the cockpit puts at its start.
    return (
      <div data-testid="scope-stub">
        <div data-testid="scope-toolbar">{p.toolbarStart as ReactNode}</div>
      </div>
    )
  },
}))
vi.mock('./BandStrip', () => ({ BandStrip: () => <div data-testid="bandstrip-stub" /> }))
vi.mock('./LogEntry', () => ({ LogEntry: () => <div data-testid="log-stub" /> }))
vi.mock('./SpotDialog', () => ({ SpotDialog: () => null }))

beforeEach(() => {
  localStorage.clear()
  settingsNow.value = {}
})
afterEach(() => {
  cleanup()
  setCwKeyer.mockClear()
  scopeProps.length = 0
})

function makeSnap(over: Record<string, unknown> = {}): AppSnapshot {
  return {
    mycall: 'KD9TAW',
    radio: {
      dialMhz: 14.05,
      band: '20m',
      catOk: true,
      sideband: 'USB',
      rigMode: 'CW',
      transmitting: false,
      txEnabled: true,
      txAllowed: true,
      cwWpm: 22,
      cwKeyer: 'cat',
      nrLevel: 0.3,
      // The rig is on FAST and says so on every read-back.
      agc: 'fast',
      refusedAgc: null,
      nb: true,
      nr: true,
      notch: null,
      filterWidthHz: 500,
      splitTxMhz: null,
      smeterDb: null,
      ...over,
    },
  } as unknown as AppSnapshot
}

async function renderCw(over: Record<string, unknown> = {}, onOpenSettings?: (where: string) => void) {
  const r = render(
    <CwCockpit snap={makeSnap(over)} theme="dark" onWorkSpot={() => {}} spots={[]} onOpenSettings={onOpenSettings} />,
  )
  await act(async () => {
    await Promise.resolve()
    await Promise.resolve()
  })
  return r
}

/** The window the scope was last handed. */
const view = () => {
  const p = scopeProps[scopeProps.length - 1]
  return { lo: p.viewLoHz as number, hi: p.viewHiHz as number }
}
const chip = (name: string) => screen.getByRole('button', { name })
const group = () => screen.getByRole('group', { name: 'Audio scope span' })

describe('the CW audio scope span', () => {
  it('opens on Std: 200–3000 Hz, with the radio’s filter drawn on it as its passband', async () => {
    await renderCw()
    expect(group()).toBeTruthy()
    expect(chip('Std').getAttribute('aria-pressed')).toBe('true')
    expect(chip('Filter').getAttribute('aria-pressed')).toBe('false')
    expect(view()).toEqual({ lo: 200, hi: 3000 })
    // The filter is still on screen: the scope is handed the reported width for its passband marks.
    expect(scopeProps[scopeProps.length - 1].passbandHz).toBe(500)
    expect(localStorage.getItem('nexus.cw.scope.span'), 'opening writes nothing').toBeNull()
  })

  it('a stored Filter pick is kept: the rig filter round the pitch, for zero-beating', async () => {
    localStorage.setItem('nexus.cw.scope.span', 'filter')
    await renderCw()
    expect(chip('Filter').getAttribute('aria-pressed')).toBe('true')
    // 500 Hz filter ⇒ 625 Hz round the 600 Hz default pitch.
    const w = cwScopeWindow(600, 500)
    expect(view()).toEqual({ lo: w.loHz, hi: w.hiHz })
    expect(view()).toEqual({ lo: 288, hi: 913 })
  })

  it('Std and Full show the waterfalls\' views, and the pick is kept for the next open', async () => {
    await renderCw()
    fireEvent.click(chip('Std'))
    expect(view()).toEqual({ lo: 200, hi: 3000 })
    expect(chip('Std').getAttribute('aria-pressed')).toBe('true')
    expect(localStorage.getItem('nexus.cw.scope.span')).toBe('std')
    fireEvent.click(chip('Full'))
    expect(view()).toEqual({ lo: 200, hi: 5000 })
    cleanup()
    await renderCw()
    expect(chip('Full').getAttribute('aria-pressed')).toBe('true')
    expect(view()).toEqual({ lo: 200, hi: 5000 })
    // And back: Filter follows the pitch and the filter again.
    fireEvent.click(chip('Filter'))
    expect(view()).toEqual({ lo: 288, hi: 913 })
    expect(localStorage.getItem('nexus.cw.scope.span')).toBe('filter')
  })

  it('a stored value no chip names opens on Std', async () => {
    localStorage.setItem('nexus.cw.scope.span', 'wide')
    await renderCw()
    expect(chip('Std').getAttribute('aria-pressed')).toBe('true')
    expect(view()).toEqual({ lo: 200, hi: 3000 })
  })

  it('the chips stand at the start of the scope\'s own toolbar, not on a row of their own', async () => {
    const { container } = await renderCw()
    // A row of their own took its height out of the picture (a 112 px panel at 1024x768).
    expect(group().closest('[data-testid="scope-toolbar"]')).not.toBeNull()
    expect(container.querySelector('.ph-scope-panel > .ph-span')).toBeNull()
  })

  it('a radio with its own scope shows the chips while its rows are sound-card audio, and applies the pick', async () => {
    // The FT-710 without its SCU-LAN10 link: the radio has a scope Nexus talks to (the span and
    // position selects), but the rows are the sound card's. That view is audio like any other, so
    // the operator can widen it, and a span picked before is the one drawn.
    localStorage.setItem('nexus.cw.scope.span', 'std')
    await renderCw({ scopeModeCode: 0x34 })
    expect(group()).toBeTruthy()
    expect(chip('Std').getAttribute('aria-pressed')).toBe('true')
    expect(view()).toEqual({ lo: 200, hi: 3000 })
    fireEvent.click(chip('Full'))
    expect(view()).toEqual({ lo: 200, hi: 5000 })
    expect(localStorage.getItem('nexus.cw.scope.span')).toBe('full')
    // The radio's own selects stay beside them.
    expect(screen.getByLabelText('Panadapter span (sets the radio)')).toBeTruthy()
  })

  it('the radio\'s own RF rows take the chips away: an audio span means nothing on them', async () => {
    await renderCw({ scopeModeCode: 0x34 })
    expect(group()).toBeTruthy()
    const onFeed = scopeProps[scopeProps.length - 1].onFeed as (s: string, lo: number, hi: number) => void
    act(() => onFeed('yaesu', 14_000_000, 14_100_000))
    expect(screen.queryByRole('group', { name: 'Audio scope span' })).toBeNull()
    // And on an Icom CI-V feed the RF zoom chips stand in their place.
    cleanup()
    await renderCw()
    const onFeed2 = scopeProps[scopeProps.length - 1].onFeed as (s: string, lo: number, hi: number) => void
    act(() => onFeed2('civ', 14_000_000, 14_100_000))
    expect(screen.queryByRole('group', { name: 'Audio scope span' })).toBeNull()
    expect(screen.getByRole('group', { name: 'Panadapter zoom' })).toBeTruthy()
  })
})

describe('the CW audio scope says when the radio\'s filter is what limits it', () => {
  const note = () => screen.queryByText(/widen it on the radio to see more/)

  it('names the filter in Std and Full, where the window is wider than it', async () => {
    await renderCw({ filterWidthHz: 500 })
    fireEvent.click(chip('Std'))
    expect(note()?.textContent).toBe("Showing your radio's 500 Hz filter — widen it on the radio to see more.")
    fireEvent.click(chip('Full'))
    expect(note()).not.toBeNull()
    // A 1 kHz filter still leaves most of Std's 2800 Hz dark.
    cleanup()
    await renderCw({ filterWidthHz: 1000 })
    fireEvent.click(chip('Std'))
    expect(note()?.textContent).toBe("Showing your radio's 1000 Hz filter — widen it on the radio to see more.")
  })

  it('never in Filter, which frames the filter by design', async () => {
    localStorage.setItem('nexus.cw.scope.span', 'filter')
    await renderCw({ filterWidthHz: 500 })
    expect(chip('Filter').getAttribute('aria-pressed')).toBe('true')
    expect(note()).toBeNull()
    // Positive control on the same render: Std names it, and back to Filter it goes.
    fireEvent.click(chip('Std'))
    expect(note()).not.toBeNull()
    fireEvent.click(chip('Filter'))
    expect(note()).toBeNull()
  })

  it('says nothing when the filter is wider than the window, unknown, or the feed is RF', async () => {
    localStorage.setItem('nexus.cw.scope.span', 'std')
    // A 3 kHz filter (the soundcard keyer's data mode) covers Std's 200–3000 Hz.
    await renderCw({ filterWidthHz: 3000 })
    expect(note()).toBeNull()
    cleanup()
    await renderCw({ filterWidthHz: null })
    expect(note()).toBeNull()
    cleanup()
    await renderCw({ filterWidthHz: 500 })
    expect(note()).not.toBeNull()
    const onFeed = scopeProps[scopeProps.length - 1].onFeed as (s: string, lo: number, hi: number) => void
    act(() => onFeed('civ', 14_000_000, 14_100_000))
    expect(note()).toBeNull()
  })
})

describe('the CW scope points an Icom at its own wide scope', () => {
  const icomOff = { rigModel: 3073, rigConn: 'serial', icomNativeCat: false, baud: 115200 }
  const hint = () => screen.queryByText(/can show its own wide scope/)
  const baudHint = () => screen.queryByText(/needs 115,200 baud/)

  it('on an IC-7300 over Hamlib: says so, and the link opens the switch', async () => {
    settingsNow.value = icomOff
    const open = vi.fn()
    await renderCw({}, open)
    expect(hint()).not.toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /Native Icom CI-V/ }))
    expect(open).toHaveBeenCalledWith('rig-advanced')
  })

  it('with the switch on below 115,200 baud: says the baud is why', async () => {
    settingsNow.value = { ...icomOff, icomNativeCat: true, baud: 38400 }
    const open = vi.fn()
    await renderCw({}, open)
    expect(hint()).toBeNull()
    expect(baudHint()).not.toBeNull()
    fireEvent.click(screen.getByRole('button', { name: /Rig & CAT/ }))
    expect(open).toHaveBeenCalledWith('rig-control')
  })

  it('says nothing where no setting would bring the scope', async () => {
    // Switch on at the right baud: the scope is Nexus's to stream.
    settingsNow.value = { ...icomOff, icomNativeCat: true }
    await renderCw({}, vi.fn())
    expect(hint()).toBeNull()
    expect(baudHint()).toBeNull()
    // The Icom network connection streams it already.
    for (const s of [
      { ...icomOff, rigConn: 'icomlan' },
      // A radio the CI-V engine does not drive.
      { ...icomOff, rigModel: 1035 },
      // The operator picked the sound card's waterfall for this radio.
      { ...icomOff, nativeScope: 'audio' },
    ]) {
      cleanup()
      settingsNow.value = s
      await renderCw({}, vi.fn())
      expect(hint()).toBeNull()
      expect(baudHint()).toBeNull()
    }
  })

  it('goes once the radio\'s own scope is streaming', async () => {
    settingsNow.value = icomOff
    await renderCw({}, vi.fn())
    expect(hint()).not.toBeNull()
    const onFeed = scopeProps[scopeProps.length - 1].onFeed as (s: string, lo: number, hi: number) => void
    act(() => onFeed('civ', 14_000_000, 14_100_000))
    expect(hint()).toBeNull()
  })

  it('a radio switch re-reads the settings: an Icom-to-Yaesu switch clears the Icom hint', async () => {
    settingsNow.value = icomOff
    const r = await renderCw({}, vi.fn())
    // Same radio re-rendered: still the Icom.
    expect(hint()).not.toBeNull()
    settingsNow.value = { rigModel: 1035, rigConn: 'serial', icomNativeCat: false, baud: 38400 }
    r.rerender(
      <CwCockpit snap={{ ...makeSnap(), activeRadioId: 2 } as AppSnapshot} theme="dark" onWorkSpot={() => {}} spots={[]} onOpenSettings={vi.fn()} />,
    )
    await act(async () => {
      await Promise.resolve()
      await Promise.resolve()
    })
    expect(hint()).toBeNull()
    expect(baudHint()).toBeNull()
  })
})
