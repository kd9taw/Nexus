// @vitest-environment jsdom
//
// THE POSITION NAME IS MANDATORY ONCE CLUB SYNC IS CONFIGURED.
//
// Club Field Day report (2026-08-30): "I started with no name, then added it but it's still
// displaying the old name/number id. Can't we make the station name a mandatory field?" Two
// halves answer that. The wire half (the name now rides every presence report, so a rename
// propagates live) is pinned in Rust. THIS is the other half: an operator who hosts or joins a
// club event with the name box empty is told, on the tab that holds the fix, instead of watching
// the club band board show something meaningless.
//
// Scoped deliberately: the refusal fires only when club sync is actually configured. A station
// that never joins a club event owes nobody a tent name, and a save it does not need must not be
// blocked by a field it will never use.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import defaultSettings from './__fixtures__/defaultSettings.json'
import type { ClubLink } from '../types'

// THE BUDGET (2026-10-09). The slowest case here, "refuses the save that TURNS HOSTING ON with the name…", takes
// 0.36 s and 0.40 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
// one core, past vitest's 5 s default. 15 s is the house budget; a test that hangs still fails, after 15 s.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Mock EVERY export of `../api`, derived from the real module (the flexperradio pattern — a
// hand-kept list makes the panel throw on mount when a verb is added).
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})
vi.mock('../toast', () => ({
  pushToast: vi.fn(),
  withErrorToast: vi.fn(async (fn: () => Promise<unknown>) => fn()),
}))

const RADIO = {
  id: 0,
  name: 'FTDX10',
  enabled: true,
  serialPort: '',
  baud: 38400,
  rigModel: 0,
  rigModelName: 'None',
  rigConn: 'serial',
  rigAddr: '',
  rigctldPort: 4532,
  rotctldPort: 4533,
  icomNativeCat: false,
  audioIn: '',
  audioOut: '',
  txLevel: 1,
  rxGain: 1,
  pttMethod: 'cat',
  rotatorModel: 0,
  rotatorPort: '',
  rotatorBaud: 9600,
  rotatorHost: '',
  nativeScope: 'auto',
  bands: [],
  flexRadioIp: '',
  flexNativePan: false,
  flexNativeAudio: false,
}

/** `club` = how this station is wired into the club event; the rest is a saveable station. */
/** A Contesting box by its caption. These fields are a <div class="settings-field">, so the
 *  caption is a plain <span> with no label association to the input, and the boxes show no
 *  placeholder to find them by. */
const findBox = async (caption: string) =>
  (await screen.findByText(caption, { selector: '.settings-label' }))
    .closest('.settings-field')!
    .querySelector('input') as HTMLInputElement
const findJoinBox = () => findBox('Join event at')
const findEventNameBox = () => findBox('Event name')

function settingsFixture(club: Record<string, unknown>) {
  return {
    ...defaultSettings,
    ...RADIO,
    mycall: 'KD9TAW',
    mygrid: 'EN52',
    activeRadio: 0,
    radios: [RADIO],
    band: '20m',
    dialMhz: 14.074,
    sideband: 'USB',
    ...club,
  } as never
}

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

function panel(clubLink?: ClubLink, onSaved?: () => void) {
  return (
    <SettingsPanel
      clubLink={clubLink}
      onSaved={onSaved}
      activeRadioId={0}
      scale={1 as never}
      scaleMode={'auto' as never}
      scaleCap={1 as never}
      onScaleModeChange={() => {}}
      onScaleCapChange={() => {}}
      density={'comfortable' as never}
      onDensityChange={() => {}}
      onResetLayout={() => {}}
      features={features}
    />
  )
}

function renderPanel(clubLink?: ClubLink, onSaved?: () => void) {
  return render(panel(clubLink, onSaved))
}

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockClear()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getAllRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('getSerialPortsDetailed').mockImplementation(() => Promise.resolve([]))
  api.get('getBandPlan').mockImplementation(() => Promise.resolve([]))
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve([]))
  api.get('getConnectionLog').mockImplementation(() => Promise.resolve([]))
  api.get('detectRigs').mockImplementation(() => Promise.resolve([]))
  api.get('appVersion').mockImplementation(() => Promise.resolve('1.6.1'))
})

afterEach(() => {
  cleanup()
  vi.restoreAllMocks()
})

/** The footer Save (`type="submit"`) — other sections carry their own "Save …" buttons. */
async function clickSave() {
  const save = (await screen.findAllByRole('button', { name: 'Save' })).find(
    (b) => (b as HTMLButtonElement).type === 'submit',
  )!
  fireEvent.click(save)
}

describe('a blank position name refuses a club-sync save', () => {
  /** The Contesting tab has to be on screen before its controls can be driven. */
  async function openContesting() {
    fireEvent.click(await screen.findByRole('tab', { name: /contesting/i }))
  }

  it('refuses the save that TURNS HOSTING ON with the name box empty, and marks the box', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdHostEnable: false, fdPositionName: '' })),
    )
    renderPanel()
    await openContesting()
    // The CHANGE is the trigger: this save is the one that starts hosting.
    fireEvent.click(await screen.findByRole('switch', { name: 'Enable club event hosting' }))
    await clickSave()

    expect(await screen.findByText(/Name this position on the Contesting tab/)).toBeTruthy()
    // Refused, not saved-then-warned: nobody starts hosting as a nameless row on the board.
    expect(api.get('setSettings')).not.toHaveBeenCalled()
    // …and the operator is told WHICH box, not just which tab. The callsign refusal has always
    // marked its field; a refusal that only names a tab is half an answer.
    const box = screen.getByLabelText(/Position name/) as HTMLInputElement
    expect(box.getAttribute('aria-invalid')).toBe('true')
  })

  it('refuses the save that SETS A JOIN ADDRESS with the name box empty', async () => {
    api.get('getSettings').mockImplementation(() =>
      // Not hosting — the join box is disabled while this position hosts its own event.
      Promise.resolve(
        settingsFixture({ fdHostEnable: false, fdJoinAddr: '', fdPositionName: '   ' }),
      ),
    )
    renderPanel()
    await openContesting()
    fireEvent.change(await findJoinBox(), {
      target: { value: '192.168.1.10:42073' },
    })
    await clickSave()

    expect(await screen.findByText(/Name this position on the Contesting tab/)).toBeTruthy()
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })

  it('refuses CLEARING a name a position already had', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdHostEnable: true, fdPositionName: 'CW tent' })),
    )
    renderPanel()
    await openContesting()
    fireEvent.change(await screen.findByLabelText(/Position name/), { target: { value: '  ' } })
    await clickSave()

    expect(await screen.findByText(/Name this position on the Contesting tab/)).toBeTruthy()
    expect(api.get('setSettings')).not.toHaveBeenCalled()
  })

  it('⭐ REGRESSION: an existing host who never named the position can still save anything else', async () => {
    // ⚠️ THE BUG THIS RULE NEARLY SHIPPED. `fdPositionName` starts empty and nothing backfills
    // it — no migration, no wizard step — so a club host running for months without one had
    // EVERY save refused by a rule that read the STATE instead of the CHANGE. The scenario is
    // 02:00 mid-event: the USB audio interface drops, the operator picks a new input device on
    // the Radio tab, presses Save, and is thrown to Contesting with the fix unsaved. It also
    // buys nothing, because an unnamed position already calls itself by its callsign on the
    // wire. This save touches nothing about club sync and must go straight through.
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdHostEnable: true, fdPositionName: '' })),
    )
    renderPanel()
    await clickSave()

    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(screen.queryByText(/Name this position on the Contesting tab/)).toBeNull()
  })

  it('POSITIVE CONTROL: a named position turning hosting on saves', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdHostEnable: false, fdPositionName: 'CW tent' })),
    )
    renderPanel()
    await openContesting()
    fireEvent.click(await screen.findByRole('switch', { name: 'Enable club event hosting' }))
    await clickSave()

    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(screen.queryByText(/Name this position on the Contesting tab/)).toBeNull()
  })

  it('POSITIVE CONTROL: a station with no club sync at all is not asked for one', async () => {
    // The scope of the rule. Blocking this save would make a Field-Day-only field mandatory for
    // every operator who never runs one.
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(
        settingsFixture({ fdHostEnable: false, fdJoinAddr: '', fdPositionName: '' }),
      ),
    )
    renderPanel()
    await clickSave()

    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(screen.queryByText(/Name this position on the Contesting tab/)).toBeNull()
  })
})

// ⭐ CLUB SYNC RUNS THE PICKED CONTEST, EXCEPT WHAT IT CANNOT RUN. The engine refuses hosting and
// joining for a contest whose merged log would be wrong whoever built it, so the Club Sync
// section says so — and why — on the tab where the switch is, naming the contest the picker
// holds.
describe('the Club Sync section says it will not run a contest the club log cannot run', () => {
  async function openContesting() {
    fireEvent.click(await screen.findByRole('tab', { name: /contesting/i }))
  }

  it('names the selected contest and the reason, before the switch is turned on', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdEvent: 'arrlss_cw', fdHostEnable: false })),
    )
    renderPanel()
    await openContesting()
    expect(
      await screen.findByText(
        /Club sync does not run ARRL November Sweepstakes \(CW\): its serial numbers must run in one sequence/,
      ),
    ).toBeTruthy()
  })

  // A downloaded rules file can drop a contest, and the engine refuses club sync for it then:
  // the station's preview of the SAVED pick resolves it to the ARRL Field Day mode entry falls
  // back to, which is how this tab can tell.
  const preview = (event: string) =>
    Promise.resolve({ event, rulesYear: 2026, bannedModes: [], spottingAllowed: true, clusterAllowed: true, enforcement: 'warn', role: '', exchange: [], problem: '' })

  it('names a contest the loaded rules do not carry, from the preview of the saved pick', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdEvent: 'ilqp', fdHostEnable: false })),
    )
    api.get('getFdRuleset').mockImplementation(() => preview('arrlfd'))
    renderPanel()
    await openContesting()
    expect(
      await screen.findByText(
        /Club sync does not run Illinois QSO Party: the contest rules this Nexus loaded do not include it/,
      ),
    ).toBeTruthy()
  })

  it('POSITIVE CONTROL: the same pick, carried by the loaded rules, carries no note', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdEvent: 'ilqp', fdHostEnable: false })),
    )
    api.get('getFdRuleset').mockImplementation(() => preview('ilqp'))
    renderPanel()
    await openContesting()
    expect(await screen.findByRole('switch', { name: /club event hosting/ })).toBeTruthy()
    await waitFor(() => expect(api.get('getFdRuleset')).toHaveBeenCalled())
    expect(screen.queryByText(/Club sync does not run/)).toBeNull()
  })

  it('POSITIVE CONTROL: the Illinois QSO Party and both Field Days carry no such note', async () => {
    for (const fdEvent of ['ilqp', 'arrlfd', 'wfd']) {
      api.get('getSettings').mockImplementation(() =>
        Promise.resolve(settingsFixture({ fdEvent, fdHostEnable: true })),
      )
      renderPanel()
      await openContesting()
      // The section is on screen (its host switch is found), and the note is not in it.
      expect(await screen.findByRole('switch', { name: /club event hosting/ })).toBeTruthy()
      expect(screen.queryByText(/Club sync does not run/)).toBeNull()
      cleanup()
    }
  })
})

// ⭐ WHERE THE ADDRESS IS TYPED, WHERE THE LINK STANDS (2026-10-10). A laptop that typed the host's
// address in Join event at said nothing about whether it was connected; the host showed no address
// to type; and an empty search said "same network?" and nothing else.
describe('the club section says where the club link stands', () => {
  async function openContesting() {
    fireEvent.click(await screen.findByRole('tab', { name: /contesting/i }))
  }
  const LINK: ClubLink = {
    role: 'position',
    addr: '192.168.1.10:42073',
    state: 'unreachable',
    why: 'refused',
    downSinceUnix: 0,
    event: '',
    hostCall: '',
    position: 'CW tent',
    clubCode: 'ABCD-EFGH',
  }

  it("says under the join address why the host is not reached, and what to check", async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdJoinAddr: '192.168.1.10', fdPositionName: 'CW tent' })),
    )
    renderPanel(LINK)
    await openContesting()
    expect(await screen.findByText('Cannot reach the host at 192.168.1.10:42073. Trying again.')).toBeTruthy()
    expect(screen.getByText(/^Nothing is listening there: check that Nexus on that laptop has Host a club event on/)).toBeTruthy()
  })

  it("gives the host the addresses a joining laptop types", async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdHostEnable: true, fdPositionName: 'HQ' })),
    )
    renderPanel({
      ...LINK,
      role: 'host',
      addr: '127.0.0.1:42073',
      state: 'joined',
      why: undefined,
      hosting: { up: true, port: 42073, addrs: [{ addr: '192.168.1.10:42073', adapter: 'Wi-Fi' }], connected: 0 },
    })
    await openContesting()
    expect(await screen.findByText(/^Hosting on port 42073\. Positions join at:/)).toBeTruthy()
    expect(screen.getByText('192.168.1.10:42073 (Wi-Fi)')).toBeTruthy()
    expect(screen.getByText('0 other positions connected.')).toBeTruthy()
  })

  it('says how long an empty search listened, what to check, and where the address to type is', async () => {
    api.get('getSettings').mockImplementation(() => Promise.resolve(settingsFixture({ fdPositionName: 'CW tent' })))
    api.get('fdDiscoverEvents').mockImplementation(() => Promise.resolve([]))
    renderPanel()
    await openContesting()
    fireEvent.click(await screen.findByRole('button', { name: 'Find club events' }))
    const empty = await screen.findByText(/^Nothing heard in 5 s\./)
    expect(empty.textContent).toContain("on Windows, that this laptop's firewall lets Nexus hear the host: press Allow Nexus through Windows Firewall, above.")
    expect(empty.textContent).toContain('the address the host shows after “Positions join at” into Join event at')
  })

  it('POSITIVE CONTROL: no club link, no line', async () => {
    api.get('getSettings').mockImplementation(() =>
      Promise.resolve(settingsFixture({ fdJoinAddr: '192.168.1.10', fdPositionName: 'CW tent' })),
    )
    renderPanel()
    await openContesting()
    await screen.findByRole('button', { name: 'Find club events' })
    expect(screen.queryByText(/the host at/)).toBeNull()
  })
})

// ⭐ A FOUND CLUB EVENT IS JOINED ON THE CLICK (2026-10-11). Two laptops on one network: the joining
// one found the host, and clicking it, "I can't tell if it's doing anything". The click only filled
// in Join event at, and nothing joined until Settings' Save. Now the click joins: it saves the join
// address alone, through the patch seam the Contest screen's switch saves through, and where the
// join stands shows right under the list.
describe('clicking a found club event joins it', () => {
  const HOST = '192.168.1.20:42073'
  const FOUND = [{ event: 'ILQP rehearsal', call: 'W9ABC', host: HOST }]
  const HOSTING_REASON = 'Hosting — this position joins its own event automatically.'

  async function openContesting() {
    fireEvent.click(await screen.findByRole('tab', { name: /contesting/i }))
  }
  /** The backend's settings, as the shell keeps them: get_settings reads them, set_settings
   *  replaces them. `elsewhere` is another writer (Remote's settings, say). */
  function backend(initial: Record<string, unknown>) {
    let stored = { ...initial }
    api.get('getSettings').mockImplementation(() => Promise.resolve({ ...stored }))
    api.get('setSettings').mockImplementation((s: Record<string, unknown>) => {
      stored = { ...s }
      return Promise.resolve(null)
    })
    return {
      now: () => stored,
      elsewhere: (patch: Record<string, unknown>) => {
        stored = { ...stored, ...patch }
      },
    }
  }
  /** Find club events, answered with FOUND; the found event's button. */
  async function findEvents() {
    api.get('fdDiscoverEvents').mockImplementation(() => Promise.resolve(FOUND))
    fireEvent.click(await screen.findByRole('button', { name: 'Find club events' }))
    return (await screen.findByRole('button', {
      name: /ILQP rehearsal — 192\.168\.1\.20:42073/,
    })) as HTMLButtonElement
  }
  /** The block holding Find club events and what it found. */
  const findBlock = () =>
    screen.getByRole('button', { name: 'Find club events' }).closest('.settings-field') as HTMLElement
  const joinBox = () => document.querySelector('input[data-setting="fdJoinAddr"]') as HTMLInputElement
  const saves = () => api.get('setSettings').mock.calls.map((c) => c[0] as Record<string, unknown>)
  const link = (over: Partial<ClubLink>): ClubLink => ({
    role: 'position',
    addr: HOST,
    state: 'looking',
    downSinceUnix: 0,
    event: '',
    hostCall: '',
    position: 'SSB tent',
    clubCode: '',
    ...over,
  })

  it('says what it does: "Join <event> — <host>", and what the click saves', async () => {
    backend(settingsFixture({ fdPositionName: 'SSB tent' }))
    renderPanel()
    await openContesting()
    const join = await findEvents()
    expect(join.textContent).toBe('Join ILQP rehearsal — 192.168.1.20:42073')
    expect(join.title).toBe(
      'Join the event at 192.168.1.20:42073 now. This saves only the join address; other changes here still need Save.',
    )
  })

  for (const fdActive of [false, true]) {
    it(`saves the join address at once, and nothing else, with Contest mode ${fdActive ? 'on' : 'off'}`, async () => {
      const live = settingsFixture({ fdActive, fdJoinAddr: '', fdPositionName: 'SSB tent' }) as Record<string, unknown>
      const station = backend(live)
      const onSaved = vi.fn()
      renderPanel(undefined, onSaved)
      await openContesting()
      fireEvent.click(await findEvents())

      await waitFor(() => expect(saves()).toHaveLength(1))
      // By value: the settings the backend held, with the join address and nothing else changed.
      expect(station.now()).toEqual({ ...live, fdJoinAddr: HOST })
      // The box shows what was saved, and the shell is told to read the station again (the line).
      await waitFor(() => expect(joinBox().value).toBe(HOST))
      expect(onSaved).toHaveBeenCalledTimes(1)
    })
  }

  it('shows where the join stands right under the list: looking, then joined, turned away or not reached', async () => {
    backend(settingsFixture({ fdJoinAddr: '', fdPositionName: 'SSB tent' }))
    const onSaved = vi.fn()
    const { rerender } = renderPanel(undefined, onSaved)
    await openContesting()
    fireEvent.click(await findEvents())
    await waitFor(() => expect(onSaved).toHaveBeenCalled())

    // The shell's next snapshots, as the link moves. Each line is the one under the list.
    const states: [Partial<ClubLink>, string][] = [
      [{ state: 'looking' }, 'Looking for the host at 192.168.1.20:42073…'],
      [
        { state: 'joined', event: 'ILQP rehearsal', hostCall: 'W9ABC' },
        'Joined ILQP rehearsal, hosted by W9ABC, as SSB tent.',
      ],
      [
        { state: 'turnedAway', told: 'Pick Illinois QSO Party as the contest.' },
        'Turned away by the host at 192.168.1.20:42073: Pick Illinois QSO Party as the contest.',
      ],
      [{ state: 'unreachable', why: 'noAnswer' }, 'Cannot reach the host at 192.168.1.20:42073. Trying again.'],
    ]
    for (const [over, line] of states) {
      rerender(panel(link(over), onSaved))
      expect(findBlock().nextElementSibling?.textContent).toBe(line)
    }
  })

  /** Stand-in for the browser's scrollIntoView (jsdom has none): the elements it was asked to show. */
  function stubReveal() {
    const reveal = vi.fn()
    const original = Element.prototype.scrollIntoView
    Element.prototype.scrollIntoView = reveal
    return {
      all: () => reveal.mock.contexts.map((el, i) => ({ el: el as Element, args: reveal.mock.calls[i] })),
      lines: () =>
        reveal.mock.contexts
          .map((el, i) => ({ el: el as Element, args: reveal.mock.calls[i] }))
          .filter((c) => c.el.matches?.('[data-club-link="link"]')),
      restore: () => {
        Element.prototype.scrollIntoView = original
      },
    }
  }

  it('brings the line the click starts into view once, and later lines never scroll the panel', async () => {
    // Measured in Chrome at 1024×768: with the found event at the panel's bottom edge, the line
    // lands 11 px under it, out of sight. 'nearest' brings it in and moves nothing when it shows.
    const reveal = stubReveal()
    try {
      backend(settingsFixture({ fdJoinAddr: '', fdPositionName: 'SSB tent' }))
      const onSaved = vi.fn()
      const { rerender } = renderPanel(undefined, onSaved)
      await openContesting()
      fireEvent.click(await findEvents())
      await waitFor(() => expect(onSaved).toHaveBeenCalled())
      rerender(panel(link({ state: 'looking' }), onSaved))
      expect(reveal.lines().map((c) => [c.el.textContent, c.args])).toEqual([
        ['Looking for the host at 192.168.1.20:42073…', [{ block: 'nearest' }]],
      ])
      // The shell's next snapshots move the line on; the panel stays where the operator puts it.
      rerender(panel(link({ state: 'joined', event: 'ILQP rehearsal', hostCall: 'W9ABC' }), onSaved))
      expect(reveal.lines()).toHaveLength(1)
    } finally {
      reveal.restore()
    }
  })

  it('CONTROL: a line that shows without a click is not brought into view', async () => {
    const reveal = stubReveal()
    try {
      backend(settingsFixture({ fdJoinAddr: HOST, fdPositionName: 'SSB tent' }))
      const { rerender } = renderPanel(link({ state: 'looking' }))
      await openContesting()
      await screen.findByText('Looking for the host at 192.168.1.20:42073…')
      rerender(panel(link({ state: 'joined', event: 'ILQP rehearsal', hostCall: 'W9ABC' })))
      expect(reveal.lines()).toHaveLength(0)
    } finally {
      reveal.restore()
    }
  })

  it('CONTROL: other unsaved edits are not saved by the click, and are not lost', async () => {
    const station = backend(
      settingsFixture({ fdJoinAddr: '', fdPositionName: 'SSB tent', fdEventName: 'Club party' }),
    )
    renderPanel()
    await openContesting()
    const eventName = await findEventNameBox()
    fireEvent.change(eventName, { target: { value: 'Unsaved edit' } })
    fireEvent.click(await findEvents())

    await waitFor(() => expect(station.now().fdJoinAddr).toBe(HOST))
    expect(station.now().fdEventName).toBe('Club party')
    expect(eventName.value).toBe('Unsaved edit')
    // …and Save still saves it, beside the join the click saved.
    await clickSave()
    await waitFor(() => expect(station.now().fdEventName).toBe('Unsaved edit'))
    expect(station.now().fdJoinAddr).toBe(HOST)
  })

  it('CONTROL: the click leaves no unsaved edit behind, so a later Save does not put its address back', async () => {
    const station = backend(
      settingsFixture({ fdJoinAddr: '', fdPositionName: 'SSB tent', fdEventName: 'Club party' }),
    )
    renderPanel()
    await openContesting()
    fireEvent.click(await findEvents())
    await waitFor(() => expect(station.now().fdJoinAddr).toBe(HOST))
    // The laptop leaves the event from elsewhere, then the operator saves an edit of their own here.
    station.elsewhere({ fdJoinAddr: '' })
    fireEvent.change(await findEventNameBox(), { target: { value: 'Unsaved edit' } })
    await clickSave()
    await waitFor(() => expect(station.now().fdEventName).toBe('Unsaved edit'))
    expect(station.now().fdJoinAddr).toBe('')
  })

  it('a laptop that is hosting cannot join one: the button is disabled, and says why beside the list', async () => {
    backend(settingsFixture({ fdHostEnable: true, fdPositionName: 'HQ' }))
    renderPanel()
    await openContesting()
    const join = await findEvents()
    expect(join.disabled).toBe(true)
    expect(within(findBlock()).getByText(HOSTING_REASON)).toBeTruthy()
    fireEvent.click(join)
    await Promise.resolve()
    expect(saves()).toHaveLength(0)
  })

  it('…and while hosting is still saved, with its switch turned off but not saved', async () => {
    // The backend ignores a join address while hosting is on (`Engine::fd_sync_targets`), so a
    // click that saved one now would join nothing until a Save turned hosting off.
    backend(settingsFixture({ fdHostEnable: true, fdPositionName: 'HQ' }))
    renderPanel(
      link({
        role: 'host',
        addr: '127.0.0.1:42073',
        state: 'joined',
        hosting: { up: true, port: 42073, addrs: [], connected: 0 },
      }),
    )
    await openContesting()
    fireEvent.click(await screen.findByRole('switch', { name: 'Disable club event hosting' }))
    expect(joinBox().disabled).toBe(false)
    const join = await findEvents()
    expect(join.disabled).toBe(true)
    expect(within(findBlock()).getByText(HOSTING_REASON)).toBeTruthy()
  })

  it('POSITIVE CONTROL: a laptop that is not hosting has the button enabled and no hosting reason', async () => {
    backend(settingsFixture({ fdHostEnable: false, fdPositionName: 'SSB tent' }))
    renderPanel()
    await openContesting()
    const join = await findEvents()
    expect(join.disabled).toBe(false)
    expect(within(findBlock()).queryByText(HOSTING_REASON)).toBeNull()
  })

  it('CONTROL: an address typed by hand and saved still joins as before', async () => {
    const station = backend(settingsFixture({ fdJoinAddr: '', fdPositionName: 'SSB tent' }))
    renderPanel()
    await openContesting()
    fireEvent.change(await findJoinBox(), {
      target: { value: '192.168.1.20' },
    })
    await Promise.resolve()
    expect(saves()).toHaveLength(0)
    await clickSave()
    await waitFor(() => expect(station.now().fdJoinAddr).toBe('192.168.1.20'))
  })

  it('a click with no position name saved is refused, as Save refuses it, and saves nothing', async () => {
    backend(settingsFixture({ fdJoinAddr: '', fdPositionName: '' }))
    renderPanel()
    await openContesting()
    fireEvent.click(await findEvents())
    expect(await within(findBlock()).findByText(/^Name this position on the Contesting tab/)).toBeTruthy()
    expect(saves()).toHaveLength(0)
    const box = screen.getByLabelText(/Position name/) as HTMLInputElement
    expect(box.getAttribute('aria-invalid')).toBe('true')
  })

  it('…the name it joins under is the saved one: a name typed and not saved is refused too', async () => {
    backend(settingsFixture({ fdJoinAddr: '', fdPositionName: '' }))
    renderPanel()
    await openContesting()
    fireEvent.change(await screen.findByLabelText(/Position name/), { target: { value: 'SSB tent' } })
    fireEvent.click(await findEvents())
    expect(await within(findBlock()).findByText(/^Name this position on the Contesting tab/)).toBeTruthy()
    expect(saves()).toHaveLength(0)
  })

  it('POSITIVE CONTROL: a laptop already joined elsewhere, with no name, moves to the found event', async () => {
    // The rule reads the change, never the state: club sync is already on here, so the click
    // turns nothing on, exactly as a Save of a new address would not be refused.
    const station = backend(settingsFixture({ fdJoinAddr: '192.168.1.30:42073', fdPositionName: '' }))
    renderPanel()
    await openContesting()
    fireEvent.click(await findEvents())
    await waitFor(() => expect(station.now().fdJoinAddr).toBe(HOST))
    expect(within(findBlock()).queryByText(/^Name this position/)).toBeNull()
  })

  // ⭐ A CONTEST CLUB SYNC CANNOT RUN (2026-10-12). The click saved the address, but the station runs
  // no club link for a contest it refuses, so nothing came under the list, and the reason was the
  // section's first line, which can be off screen. That sentence now also comes under the list after
  // the click, where the line would have.
  describe('for a contest club sync cannot run', () => {
    const NOT_LOADED =
      'Club sync does not run Illinois QSO Party: the contest rules this Nexus loaded do not include it. Check for rules updates on the Contesting tab in Settings (new rules apply when Nexus starts again), or pick another contest. This station does not host or join a club event, and each position keeps its own log.'
    const SERIAL =
      'Club sync does not run ARRL November Sweepstakes (CW): its serial numbers must run in one sequence for the whole entry, and every position would give out its own. This station does not host or join a club event, and each position keeps its own log.'
    /** The station's preview of the saved pick: the contest its loaded rules resolve that pick to. */
    const rulesResolveTo = (event: string) =>
      api.get('getFdRuleset').mockImplementation(() =>
        Promise.resolve({ event, rulesYear: 2026, bannedModes: [], spottingAllowed: true, clusterAllowed: true, enforcement: 'warn', role: '', exchange: [], problem: '' }),
      )
    const contestChip = (name: string) =>
      [...document.querySelectorAll('button.theme-chip')].find((b) => b.textContent === name) as HTMLButtonElement

    // The operator's case (the loaded rules lack the contest), and a contest no club log can run.
    for (const [fdEvent, resolved, sentence] of [
      ['ilqp', 'arrlfd', NOT_LOADED],
      ['arrlss_cw', 'arrlss_cw', SERIAL],
    ]) {
      it(`a click says why right under the list, in the section's own sentence (${fdEvent})`, async () => {
        const live = settingsFixture({ fdEvent, fdJoinAddr: '', fdPositionName: 'SSB tent' }) as Record<string, unknown>
        const station = backend(live)
        rulesResolveTo(resolved)
        const onSaved = vi.fn()
        const { rerender } = renderPanel(undefined, onSaved)
        await openContesting()
        await screen.findByText(sentence)
        fireEvent.click(await findEvents())
        await waitFor(() => expect(onSaved).toHaveBeenCalled())
        // The station's next snapshot: club sync refused, so no club link (`Engine::fd_club_link`).
        rerender(panel(undefined, onSaved))
        expect(findBlock().nextElementSibling?.textContent).toBe(sentence)
        // The section's first line stays: the sentence there and under the list.
        expect(screen.getAllByText(sentence)).toHaveLength(2)
        // The click still saved the join address and nothing else.
        expect(station.now()).toEqual({ ...live, fdJoinAddr: HOST })
      })
    }

    it('CONTROL: no click, no sentence under the list; the section says it once', async () => {
      backend(settingsFixture({ fdEvent: 'ilqp', fdJoinAddr: '', fdPositionName: 'SSB tent' }))
      rulesResolveTo('arrlfd')
      renderPanel()
      await openContesting()
      await findEvents()
      expect(await screen.findByText(NOT_LOADED)).toBeTruthy()
      expect(screen.getAllByText(NOT_LOADED)).toHaveLength(1)
      expect(findBlock().nextElementSibling?.textContent).not.toBe(NOT_LOADED)
    })

    it('CONTROL: the same contest, carried by the loaded rules, joins as before, with no refusal', async () => {
      backend(settingsFixture({ fdEvent: 'ilqp', fdJoinAddr: '', fdPositionName: 'SSB tent' }))
      rulesResolveTo('ilqp')
      const onSaved = vi.fn()
      const { rerender } = renderPanel(undefined, onSaved)
      await openContesting()
      fireEvent.click(await findEvents())
      await waitFor(() => expect(onSaved).toHaveBeenCalled())
      expect(api.get('getFdRuleset')).toHaveBeenCalled()
      rerender(panel(link({ state: 'looking' }), onSaved))
      expect(findBlock().nextElementSibling?.textContent).toBe('Looking for the host at 192.168.1.20:42073…')
      rerender(panel(link({ state: 'joined', event: 'ILQP rehearsal', hostCall: 'W9ABC' }), onSaved))
      expect(findBlock().nextElementSibling?.textContent).toBe('Joined ILQP rehearsal, hosted by W9ABC, as SSB tent.')
      expect(screen.queryByText(/Club sync does not run/)).toBeNull()
    })

    it('CONTROL: a hosting laptop still cannot click one, so the sentence stays the section\'s alone', async () => {
      backend(settingsFixture({ fdEvent: 'ilqp', fdHostEnable: true, fdPositionName: 'HQ' }))
      rulesResolveTo('arrlfd')
      renderPanel()
      await openContesting()
      const join = await findEvents()
      expect(join.disabled).toBe(true)
      expect(within(findBlock()).getByText(HOSTING_REASON)).toBeTruthy()
      fireEvent.click(join)
      await Promise.resolve()
      expect(saves()).toHaveLength(0)
      expect(await screen.findByText(NOT_LOADED)).toBeTruthy()
      expect(screen.getAllByText(NOT_LOADED)).toHaveLength(1)
    })

    it('CONTROL: a click refused for the position name saves nothing, so only that refusal comes', async () => {
      backend(settingsFixture({ fdEvent: 'ilqp', fdJoinAddr: '', fdPositionName: '' }))
      rulesResolveTo('arrlfd')
      renderPanel()
      await openContesting()
      await screen.findByText(NOT_LOADED)
      fireEvent.click(await findEvents())
      expect(await within(findBlock()).findByText(/^Name this position on the Contesting tab/)).toBeTruthy()
      expect(saves()).toHaveLength(0)
      expect(screen.getAllByText(NOT_LOADED)).toHaveLength(1)
    })

    it('brings the sentence into view once, as it would the line, and a later line moves nothing', async () => {
      const reveal = stubReveal()
      try {
        backend(settingsFixture({ fdEvent: 'ilqp', fdJoinAddr: '', fdPositionName: 'SSB tent' }))
        rulesResolveTo('arrlfd')
        const onSaved = vi.fn()
        const { rerender } = renderPanel(undefined, onSaved)
        await openContesting()
        await screen.findByText(NOT_LOADED)
        fireEvent.click(await findEvents())
        await waitFor(() => expect(onSaved).toHaveBeenCalled())
        rerender(panel(undefined, onSaved))
        const under = findBlock().nextElementSibling
        expect(reveal.all().map((c) => [c.el.textContent, c.el === under, c.args])).toEqual([
          [NOT_LOADED, true, [{ block: 'nearest' }]],
        ])
        // The click's reveal is spent: a line a later snapshot brings is not scrolled to.
        rerender(panel(link({ state: 'looking' }), onSaved))
        expect(reveal.all()).toHaveLength(1)
      } finally {
        reveal.restore()
      }
    })

    it('names the contest the join runs under, the saved one: the click does not save the picker', async () => {
      const live = settingsFixture({ fdEvent: 'ilqp', fdJoinAddr: '', fdPositionName: 'SSB tent' }) as Record<string, unknown>
      const station = backend(live)
      rulesResolveTo('arrlfd')
      const onSaved = vi.fn()
      const { rerender } = renderPanel(undefined, onSaved)
      await openContesting()
      await screen.findByText(NOT_LOADED)
      // Picked and not saved: the section's first line speaks for the picker, so it goes.
      fireEvent.click(contestChip('ARRL Field Day'))
      expect(screen.queryByText(NOT_LOADED)).toBeNull()
      fireEvent.click(await findEvents())
      await waitFor(() => expect(onSaved).toHaveBeenCalled())
      rerender(panel(undefined, onSaved))
      expect(station.now()).toEqual({ ...live, fdJoinAddr: HOST })
      expect(findBlock().nextElementSibling?.textContent).toBe(NOT_LOADED)
    })
  })
})
