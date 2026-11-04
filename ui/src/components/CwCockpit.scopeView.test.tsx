// @vitest-environment jsdom
//
// THE CW SCOPE VIEWS on a radio whose own scope Nexus draws (an Icom over native CI-V, a Yaesu over
// its LAN scope link, a FlexRadio): Both by default, the radio's scope as the wide view with the
// sound-card audio view under it; an RF / Audio / Both chip in the scope head, kept per window and
// validated on load; one span control that owns the radio's span; the audio view reading the audio
// only, so it keeps working when the radio's scope stalls; and the slow-scope hint.
import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest'
import { render, screen, cleanup, fireEvent, act, within } from '@testing-library/react'
import { CwCockpit } from './CwCockpit'
import * as api from '../api'
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
    // The scope's own toolbar strip, holding whatever the cockpit puts at its start, tagged with
    // the feed it reads so a test can tell the radio's pane from the audio one.
    return (
      <div data-testid="scope-stub" data-feed={(p.feed as string | undefined) ?? 'scope'}>
        <div data-testid="scope-toolbar">{p.toolbarStart as ReactNode}</div>
      </div>
    )
  },
}))
// The scope's divider records the default height it was handed (its own clamping is PaneSeam's tests').
const { seamDefaults } = vi.hoisted(() => ({ seamDefaults: [] as (number | null)[] }))
vi.mock('./PaneSeam', () => ({
  PaneSeam: (p: { defaultPct: number | null; varName?: string }) => {
    if (p.varName === '--cw-scope-h') seamDefaults.push(p.defaultPct)
    return <div className="pane-splitter horizontal" />
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
  document.documentElement.style.removeProperty('--vh-eff')
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


/** The scopes on screen, by the feed each reads. */
const feeds = () => [...document.querySelectorAll('[data-testid="scope-stub"]')].map((e) => e.getAttribute('data-feed'))
/** The props the newest scope reading `feed` was handed. */
const propsOf = (feed: string) => {
  for (let i = scopeProps.length - 1; i >= 0; i--) if (((scopeProps[i].feed as string | undefined) ?? 'scope') === feed) return scopeProps[i]
  throw new Error(`no scope reads ${feed}`)
}
type Report = (s: string, lo: number, hi: number) => void
/** The radio's scope streams: the rig scope (the one view before a radio scope is seen) says so. */
const radioScopeSeen = (source = 'civ') => act(() => (propsOf('scope').onFeed as Report)(source, 14_025_000, 14_075_000))
const viewChip = (name: string) =>
  within(screen.getByRole('group', { name: 'What the scope shows' })).getByRole('button', { name })
const spanCalls = () => vi.mocked(api.setScopeSpan).mock.calls.length + vi.mocked(api.setFlexPanSpan).mock.calls.length

describe('the CW scope on a radio with its own scope', () => {
  beforeEach(() => {
    seamDefaults.length = 0
    // A window tall enough for the halves to stack (useViewport publishes --vh-eff; jsdom has none).
    document.documentElement.style.setProperty('--vh-eff', '900px')
    vi.mocked(api.setScopeSpan).mockClear()
    vi.mocked(api.setFlexPanSpan).mockClear()
  })

  it('draws the one rig scope, and offers no view chip, until a radio scope has been seen', async () => {
    await renderCw()
    expect(feeds()).toEqual(['scope'])
    expect(screen.queryByRole('group', { name: 'What the scope shows' })).toBeNull()
    // A sound-card row is not a radio scope (control).
    act(() => (propsOf('scope').onFeed as Report)('audio', 200, 3000))
    expect(feeds()).toEqual(['scope'])
    expect(screen.queryByRole('group', { name: 'What the scope shows' })).toBeNull()
  })

  it('defaults to BOTH: the radio scope over the audio view, two fill panes in the strip, opening at ±25k', async () => {
    await renderCw()
    radioScopeSeen()
    expect(feeds()).toEqual(['rf', 'audio'])
    expect(viewChip('Both').getAttribute('aria-pressed')).toBe('true')
    const stack = document.querySelector('.ph-scope-panel > .scope-stack')!
    expect(stack, 'the stack is inside the scope strip').not.toBeNull()
    const frames = [...stack.children]
    expect(frames.map((f) => f.getAttribute('data-pane'))).toEqual(['scopeRf', 'scopeAudio'])
    for (const f of frames) {
      expect(f.classList.contains('pane-frame')).toBe(true)
      expect(f.getAttribute('data-fit')).toBe('fill')
    }
    // The radio's pane draws the radio's scope at the chosen span; the audio pane draws the audio
    // window with the pitch marker, and carries the audio span chips.
    expect(within(screen.getByRole('group', { name: 'Panadapter zoom' })).getByRole('button', { name: '±25k' }).getAttribute('aria-pressed')).toBe('true')
    expect([propsOf('rf').viewLoHz, propsOf('rf').viewHiHz]).toEqual([-25_000, 25_000])
    expect(propsOf('audio').markerHz).toBeTypeOf('number')
    expect(propsOf('audio').viewHiHz).toBeLessThanOrEqual(5000)
    const audioPane = document.querySelector('[data-pane="scopeAudio"]') as HTMLElement
    expect(within(audioPane).getByRole('group', { name: 'Audio scope span' })).toBeTruthy()
    // Each pane is the frame's, never a size of its own: no class or style beyond the role.
    expect(frames.every((f) => f.getAttribute('class') === 'pane-frame')).toBe(true)
  })

  it('a stored Band pick is kept: ±25k is only the default', async () => {
    localStorage.setItem('nexus.cw.rfSpan', 'band')
    await renderCw()
    radioScopeSeen()
    expect(within(screen.getByRole('group', { name: 'Panadapter zoom' })).getByRole('button', { name: 'Band' }).getAttribute('aria-pressed')).toBe('true')
  })

  it('the chip round trip: Audio, RF and Both, each kept for the next open of this window', async () => {
    await renderCw()
    radioScopeSeen()
    fireEvent.click(viewChip('Audio'))
    expect(feeds()).toEqual(['audio'])
    expect(localStorage.getItem('nexus.cw.scopeView')).toBe('audio')
    // The audio view has its chips and no radio span chips.
    expect(screen.getByRole('group', { name: 'Audio scope span' })).toBeTruthy()
    expect(screen.queryByRole('group', { name: 'Panadapter zoom' })).toBeNull()
    fireEvent.click(viewChip('RF'))
    expect(feeds()).toEqual(['scope'])
    expect(localStorage.getItem('nexus.cw.scopeView')).toBe('rf')
    fireEvent.click(viewChip('Both'))
    expect(feeds()).toEqual(['rf', 'audio'])
    expect(localStorage.getItem('nexus.cw.scopeView')).toBe('both')

    // Reopened with Audio stored: the rig scope first (nothing seen yet), then the Audio view.
    cleanup()
    localStorage.setItem('nexus.cw.scopeView', 'audio')
    await renderCw()
    expect(feeds()).toEqual(['scope'])
    radioScopeSeen()
    expect(feeds()).toEqual(['audio'])
    expect(viewChip('Audio').getAttribute('aria-pressed')).toBe('true')
  })

  it('validates the stored view on load: a word it does not offer opens on Both', async () => {
    for (const bad of ['bogus', 'BOTH', '', 'rf ']) {
      localStorage.setItem('nexus.cw.scopeView', bad)
      await renderCw()
      radioScopeSeen()
      expect(feeds(), `stored ${JSON.stringify(bad)}`).toEqual(['rf', 'audio'])
      cleanup()
    }
  })

  it('a stall of the radio scope leaves the audio view running, and the radio pane in place to say so', async () => {
    await renderCw()
    radioScopeSeen()
    // The radio pane reads the radio only (it shows "No scope data from the radio" when that
    // stops, PhoneScope.rfview); the audio pane reads the audio only, never the radio's row.
    expect(propsOf('rf').feed).toBe('rf')
    expect(propsOf('audio').feed).toBe('audio')
    // Nothing the radio pane does or does not report takes the stack down: a fresh snapshot with no
    // new rows keeps both panes.
    act(() => (propsOf('rf').onFeed as Report)('civ', 14_025_000, 14_075_000))
    expect(feeds()).toEqual(['rf', 'audio'])
  })

  it('ONE span owner: the radio span chips set the span, once, and no second control commands it', async () => {
    const r = await renderCw()
    radioScopeSeen()
    // No rig-span row anywhere: the Rig scope pane keeps only its Ref level.
    expect(screen.queryAllByTitle(/scope span to|panadapter bandwidth to/)).toEqual([])
    const sweep = propsOf('rf').onSweep as Report
    // The radio sweeps ±50k around the dial; ±25k fits in it, so nothing is asked.
    act(() => sweep('civ', 14_000_000, 14_100_000))
    expect(spanCalls()).toBe(0)
    // ±10k does not: one ask for the ±10k rung, and only one however often the scope reports.
    fireEvent.click(within(screen.getByRole('group', { name: 'Panadapter zoom' })).getByRole('button', { name: '±10k' }))
    act(() => sweep('civ', 14_000_000, 14_100_000))
    await act(async () => {
      await Promise.resolve()
    })
    act(() => sweep('civ', 14_000_000, 14_100_000))
    expect(vi.mocked(api.setScopeSpan).mock.calls).toEqual([[10_000]])
    // The radio stays on ±50k (an Icom in Fixed mode refuses the ask). An over and its unkey run the
    // ask again, and it is not sent again: the radio's own span stands, told once.
    const cockpit = (over: Record<string, unknown>) => (
      <CwCockpit snap={makeSnap(over)} theme="dark" onWorkSpot={() => {}} spots={[]} />
    )
    r.rerender(cockpit({ transmitting: true }))
    r.rerender(cockpit({ transmitting: false }))
    act(() => sweep('civ', 14_000_000, 14_100_000))
    expect(spanCalls()).toBe(1)
  })

  it('keeps the Ref level of the radio scope', async () => {
    await renderCw()
    radioScopeSeen()
    expect(screen.getByRole('slider', { name: 'Scope reference level (dB)' })).toBeTruthy()
  })

  it('Stop TX and the dock stay where they are, unmounted by nothing, when the view changes', async () => {
    await renderCw()
    radioScopeSeen()
    const stop = [...document.querySelectorAll('.cockpit-txstrip button')].find((b) => /^stop tx$/i.test(b.textContent!.trim()))!
    const dock = document.querySelector('.cockpit-txdock')!
    expect(stop).toBeTruthy()
    const shell = dock.parentElement!
    const placeOf = (el: Element) => [...shell.children].indexOf(el.closest(':scope > *') ?? el)
    const before = { strip: [...shell.children].indexOf(stop.closest('.cockpit-txstrip')!), dock: [...shell.children].indexOf(dock) }
    for (const name of ['Audio', 'RF', 'Both']) {
      fireEvent.click(viewChip(name))
      expect(stop.isConnected, `Stop TX was remounted by ${name}`).toBe(true)
      expect(dock.isConnected, `the dock was remounted by ${name}`).toBe(true)
      expect([...shell.children].indexOf(stop.closest('.cockpit-txstrip')!)).toBe(before.strip)
      expect([...shell.children].indexOf(dock)).toBe(before.dock)
      expect(placeOf(dock)).toBe(before.dock)
    }
  })

  it('says set the radio\'s scope SPEED to FAST while an Icom scope arrives slowly, and only then', async () => {
    await renderCw()
    radioScopeSeen()
    const hint = () => screen.queryByText("Set the radio's scope SPEED to FAST for a smoother scope")
    const rate = (r: number | null) => act(() => (propsOf('rf').onSweepRate as (r: number | null) => void)(r))
    expect(hint()).toBeNull()
    rate(2.9)
    expect(hint()).not.toBeNull()
    rate(12)
    expect(hint()).toBeNull()
    rate(null)
    expect(hint()).toBeNull()
    // Not on a FlexRadio: its pan has no SPEED menu.
    cleanup()
    await renderCw()
    radioScopeSeen('flex')
    act(() => (propsOf('rf').onSweepRate as (r: number | null) => void)(2))
    expect(hint()).toBeNull()
    // Nothing goes to the radio for it.
    expect(spanCalls()).toBe(0)
  })

  it('the strip opens at 40 % once a radio scope is seen, 25 % otherwise, and never changes on the air', async () => {
    const lastDefault = () => seamDefaults[seamDefaults.length - 1]
    // Control: a radio with no scope of its own opens at every waterfall's 25 %.
    await renderCw()
    expect(lastDefault()).toBe(25)
    act(() => (propsOf('scope').onFeed as Report)('audio', 200, 3000))
    expect(lastDefault()).toBe(25)
    radioScopeSeen()
    expect(lastDefault()).toBe(40)
    // The same in every view: the chip moves nothing.
    for (const name of ['Audio', 'RF', 'Both']) {
      fireEvent.click(viewChip(name))
      expect(lastDefault(), name).toBe(40)
    }
    cleanup()
    // Seen mid-over: the strip waits for the unkey.
    const cockpit = (over: Record<string, unknown>) => (
      <CwCockpit snap={makeSnap(over)} theme="dark" onWorkSpot={() => {}} spots={[]} />
    )
    const r = await renderCw({ transmitting: true })
    radioScopeSeen()
    expect(lastDefault()).toBe(25)
    r.rerender(cockpit({ transmitting: true, rigKeyed: true }))
    expect(lastDefault()).toBe(25)
    r.rerender(cockpit({ transmitting: false }))
    expect(lastDefault()).toBe(40)
  })

  it('the stacked halves are compact, each toolbar in its pane head, and the span chips ride the radio pane', async () => {
    await renderCw()
    radioScopeSeen()
    expect(propsOf('rf').compact).toBe(true)
    expect(propsOf('audio').compact).toBe(true)
    const rfHost = propsOf('rf').toolbarHost as HTMLElement
    const audioHost = propsOf('audio').toolbarHost as HTMLElement
    expect(rfHost.closest('.pane-head')?.closest('[data-pane="scopeRf"]')).toBeTruthy()
    expect(audioHost.closest('.pane-head')?.closest('[data-pane="scopeAudio"]')).toBeTruthy()
    // One span row, the radio pane's own; none left in the strip above the panes.
    expect(screen.getAllByRole('group', { name: 'Panadapter zoom' }).length).toBe(1)
    expect(document.querySelector('.ph-scope-panel > .ph-span[aria-label="Panadapter zoom"]')).toBeNull()
    // The single views are not compact.
    fireEvent.click(viewChip('RF'))
    expect(propsOf('scope').compact).toBeFalsy()
  })

  it("the slow hint reads the station's SPEED report when there is one", async () => {
    const hint = () => screen.queryByText("Set the radio's scope SPEED to FAST for a smoother scope")
    await renderCw({ scopeRate: { sweepsPerSec: 9, dropped: 0, speed: 'slow' } })
    radioScopeSeen()
    expect(hint()).not.toBeNull()
    cleanup()
    // FAST read back: no hint, whatever this scope measured.
    await renderCw({ scopeRate: { sweepsPerSec: 2, dropped: 3, speed: 'fast' } })
    radioScopeSeen()
    act(() => (propsOf('rf').onSweepRate as (r: number | null) => void)(1))
    expect(hint()).toBeNull()
  })

  it('stacks the halves on a tall enough window, and stands them side by side on a short one', async () => {
    const stack = () => document.querySelector('.ph-scope-panel > .scope-stack')!
    document.documentElement.style.setProperty('--vh-eff', '900px')
    await renderCw()
    radioScopeSeen()
    expect(stack().classList.contains('scope-stack--beside')).toBe(false)
    cleanup()
    // 1366×768 at 175 %: the effective window is 439 px tall.
    document.documentElement.style.setProperty('--vh-eff', '439px')
    await renderCw()
    radioScopeSeen()
    expect(stack().classList.contains('scope-stack--beside')).toBe(true)
    // The same panes either way, in the same order.
    expect([...stack().children].map((f) => f.getAttribute('data-pane'))).toEqual(['scopeRf', 'scopeAudio'])
    // There the radio's span is one select, the same choice as the chips, kept as they keep it.
    const select = within(screen.getByRole('group', { name: 'Panadapter zoom' })).getByRole('combobox')
    expect((select as HTMLSelectElement).value).toBe('±25k')
    fireEvent.change(select, { target: { value: '±10k' } })
    expect(localStorage.getItem('nexus.cw.rfSpan')).toBe('±10k')
    expect([propsOf('rf').viewLoHz, propsOf('rf').viewHiHz]).toEqual([-10_000, 10_000])
  })
})

