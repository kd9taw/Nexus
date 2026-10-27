// @vitest-environment jsdom
//
// SETTINGS ▸ CONTESTING, IN THE ORDER AN ENTRY IS SET UP: the contest and the switch that starts
// it, what the station sends, what the log declares, the aids, Field Day's own fields, the club.
//
// The tab had grown by accretion. The entry's Cabrillo declarations sat under the contest picker;
// the Unassisted switch, the other half of "assisted", had a group of its own; and the switch that
// starts EVERY contest shared a group with Field Day's class, section and power multiplier, under
// the name "Contest mode". Every control keeps its stored key and what it does; only its group
// moved. What this file holds, by value:
//
//  1. the groups, in order, and the group each control sits in;
//  2. Field Day's group only for the two Field Days, and its power multiplier only for ARRL's;
//  3. Power sent only for a contest whose exchange sends the station's power — none of the
//     shipped ones does (`CONTESTS_SENDING_POWER`, checked against the rules seed in Rust);
//  4. a whole stored settings file, every Contesting value set: each control shows its value,
//     and a Save that changed nothing writes the same file back.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { FieldDayStatus } from '../types'
import { CONTESTS, CONTESTS_SENDING_POWER, isFieldDay } from '../fdEvent'
import defaultSettings from './__fixtures__/defaultSettings.json'
// ⭐ A whole settings file as this release's Rust `Settings` serialises it (every key, 335 of
// them), with every Contesting value set away from its default (the Cabrillo declarations
// included) and the Station tab's State set to a ragchew QTH. Made by serialising `Settings` with those values; nothing in it is typed by
// hand but the two ESM step maps, whose keys the Rust side keeps as written.
import storedFile from './__fixtures__/settings-contesting-stored.json'

// THE BUDGET: the house's 15 s. The round trips render the whole panel and walk every tab group;
// a loaded full suite on this box has run cases up to 20 times slower than one core.
vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})

// Every export of `../api`, derived from the real module (the contest-pick suite's reason).
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

const features: FeaturesApi = {
  enabled: () => true,
  setEnabled: vi.fn(),
  all: () => [],
  profile: 'full',
  setProfile: vi.fn(),
} as unknown as FeaturesApi

/** `fieldDay` is the live snapshot's contest block: a running session, or none. */
function renderPanel(fieldDay: FieldDayStatus | null = null) {
  return render(
    <SettingsPanel
      fieldDay={fieldDay}
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
    />,
  )
}

/** The settings the station holds; a Save replaces them, as the backend does. */
let station: Record<string, unknown> = {}

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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.3'))
  api.get('getAssistanceJournal').mockImplementation(() => Promise.resolve([]))
  station = { ...defaultSettings, mycall: 'W9XYZ', mygrid: 'EN52' }
  api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(station) as never))
  api.get('setSettings').mockImplementation((s: unknown) => {
    station = structuredClone(s as Record<string, unknown>)
    return Promise.resolve(null)
  })
})
afterEach(cleanup)

const openTab = async (name: string) => fireEvent.click(await screen.findByRole('tab', { name }))

/** The legend of the group a control sits in. */
const groupOf = (el: Element | null) =>
  el?.closest('fieldset')?.querySelector('legend')?.textContent ?? '(not on the tab)'

/** Every group's legend on the tab, in order. */
const legends = () =>
  [...document.querySelectorAll('fieldset.settings-section > legend')].map((n) => n.textContent)

/** The field a label names (`.settings-label` text, exactly), or null. */
const fieldLabelled = (label: string) =>
  [...document.querySelectorAll('.settings-label')]
    .find((n) => n.textContent === label)
    ?.closest('.settings-field') ?? null

/** The input or select a label names. */
const boxLabelled = (label: string) =>
  fieldLabelled(label)?.querySelector('input, select') as HTMLInputElement | HTMLSelectElement | null

/** The pressed chip of a chip group, by the group's accessible name. */
const pressedIn = (group: string) =>
  [...screen.getByRole('group', { name: group }).querySelectorAll('button')].find(
    (b) => b.getAttribute('aria-pressed') === 'true',
  )?.textContent ?? null

const chipFor = (contest: string) =>
  within(screen.getByRole('group', { name: 'Which contest you are entering' })).getByRole('button', {
    name: contest,
  })

// The Cabrillo declarations each contest's rules data gives the preview, as the seed has them:
// CQ WW CW has all five (its band category too), the Illinois QSO Party the mailing address and
// the soapbox, and the two Field Days none. The class is offered to a MULTI-OP entry only.
const DECLARES: Record<string, Record<string, unknown>> = {
  cqww_cw: {
    bandCategories: ['ALL', '160M', '80M', '40M', '20M', '15M', '10M'],
    transmitterCategories: ['ONE', 'TWO', 'UNLIMITED'],
    overlayCategories: ['CLASSIC', 'ROOKIE', 'YOUTH'],
    addressHeaders: ['ADDRESS', 'ADDRESS-CITY', 'ADDRESS-STATE-PROVINCE', 'ADDRESS-POSTALCODE', 'ADDRESS-COUNTRY'],
    soapbox: true,
  },
  ilqp: {
    addressHeaders: ['ADDRESS', 'ADDRESS-CITY', 'ADDRESS-STATE-PROVINCE', 'ADDRESS-POSTALCODE'],
    soapbox: true,
  },
}
const CLASS = 'Which multi-operator class you are entering'
const OVERLAY = 'Which overlay category you are also entering'
const BAND = 'Which band category you are entering'

const LEGENDS = [
  'Contest',
  'Your station data',
  'Your entry',
  'Super Check Partial and call history',
  'Enter Sends Message',
  'Field Day',
  "Who's who at this event",
  'Field Day Club Sync',
]

describe('the groups, in the order an entry is set up', () => {
  it('Contest, station data, entry, the aids, Field Day, the club', async () => {
    renderPanel()
    await openTab('Contesting')
    await screen.findByRole('group', { name: 'Which contest you are entering' })
    expect(legends()).toEqual(LEGENDS)
  })

  it('a contest that is not a Field Day has no Field Day group, and nothing else moves', async () => {
    station = { ...station, fdEvent: 'ilqp' }
    renderPanel()
    await openTab('Contesting')
    await screen.findByRole('group', { name: 'Which contest you are entering' })
    expect(legends()).toEqual(LEGENDS.filter((l) => l !== 'Field Day'))
  })

  it('puts every control in its group', async () => {
    station = { ...station, fdScoreboard: true }
    renderPanel()
    await openTab('Contesting')
    await screen.findByRole('group', { name: 'Which contest you are entering' })
    const where: Record<string, Element | null> = {
      'the contest picker': screen.getByRole('group', { name: 'Which contest you are entering' }),
      'Your role': fieldLabelled('Your role'),
      // Start contest… / Stop contest…: it opens the setup or the end-of-contest checklist; the form
      // no longer saves the mode.
      'the Contest mode switch': screen.getByRole('button', { name: /^(Start|Stop) contest…$/ }),
      'Check for rules updates': screen.getByRole('button', { name: 'Check for rules updates' }),
      County: boxLabelled('County'),
      'State or province': boxLabelled('State or province'),
      Check: boxLabelled('Check'),
      'CQ zone': boxLabelled('CQ zone'),
      'ITU zone': boxLabelled('ITU zone'),
      'Entry category': screen.getByRole('group', { name: 'How many operators are at this station' }),
      'Power category': screen.getByRole('group', { name: 'Which power category you are entering' }),
      'Spotting assistance': screen.getByRole('group', { name: 'Whether your entry uses spotting assistance' }),
      'Station type': screen.getByRole('group', { name: 'Whether this is a school club station' }),
      'Unassisted entry': screen.getByRole('switch', { name: 'Declare an unassisted contest entry' }),
      'Email for contest logs': boxLabelled('Email for contest logs'),
      Club: boxLabelled('Club'),
      'Other operators': boxLabelled('Other operators'),
      'Super Check Partial': screen.getByRole('switch', { name: 'Super Check Partial' }),
      'Fill the exchange from call history': screen.getByRole('switch', { name: 'Fill the exchange from call history' }),
      'ESM in the CW cockpit': screen.getByRole('switch', { name: 'ESM in the CW cockpit' }),
      'Call once (S&P)': screen.getByRole('switch', { name: 'Call once (S&P)' }),
      'FD Class': boxLabelled('FD Class'),
      'ARRL Section': boxLabelled('ARRL Section'),
      'Power multiplier': screen.getByRole('group', { name: 'Field Day power multiplier' }),
      'Callsign on the air': boxLabelled('Callsign on the air'),
      'Position name': boxLabelled('Position name'),
      'Operator at the key': boxLabelled('Operator at the key'),
      'Host a club event': screen.getByRole('switch', { name: 'Enable club event hosting' }),
      'Join event at': boxLabelled('Join event at'),
      'Spectator scoreboard': screen.getByRole('switch', { name: 'Disable the spectator scoreboard' }),
    }
    const placed = Object.fromEntries(Object.entries(where).map(([name, el]) => [name, groupOf(el)]))
    expect(placed).toEqual({
      'the contest picker': 'Contest',
      'Your role': 'Contest',
      'the Contest mode switch': 'Contest',
      'Check for rules updates': 'Contest',
      County: 'Your station data',
      'State or province': 'Your station data',
      Check: 'Your station data',
      'CQ zone': 'Your station data',
      'ITU zone': 'Your station data',
      'Entry category': 'Your entry',
      'Power category': 'Your entry',
      'Spotting assistance': 'Your entry',
      'Station type': 'Your entry',
      'Unassisted entry': 'Your entry',
      'Email for contest logs': 'Your entry',
      Club: 'Your entry',
      'Other operators': 'Your entry',
      'Super Check Partial': 'Super Check Partial and call history',
      'Fill the exchange from call history': 'Super Check Partial and call history',
      'ESM in the CW cockpit': 'Enter Sends Message',
      'Call once (S&P)': 'Enter Sends Message',
      'FD Class': 'Field Day',
      'ARRL Section': 'Field Day',
      'Power multiplier': 'Field Day',
      'Callsign on the air': "Who's who at this event",
      'Position name': "Who's who at this event",
      'Operator at the key': "Who's who at this event",
      'Host a club event': 'Field Day Club Sync',
      'Join event at': 'Field Day Club Sync',
      'Spectator scoreboard': 'Field Day Club Sync',
    })
  })

  it('the Illinois QSO Party\'s entry class sits with the rest of the entry', async () => {
    station = { ...station, fdEvent: 'ilqp' }
    api.get('getFdRuleset').mockImplementation(() =>
      Promise.resolve({ event: 'ilqp', entryClasses: ['ILLINOIS MOBILE', 'UNLIMITED'] } as never),
    )
    renderPanel()
    await openTab('Contesting')
    await waitFor(() => expect(boxLabelled('Entry class')).not.toBeNull())
    expect(groupOf(boxLabelled('Entry class'))).toBe('Your entry')
  })

  it('a multi-operator class, a band category, an overlay, the mailing address and the soapbox sit with the rest of the entry', async () => {
    station = { ...station, fdEvent: 'cqww_cw', contestCategoryOperator: 'MULTI-OP' }
    api.get('getFdRuleset').mockImplementation(() =>
      Promise.resolve({ event: 'cqww_cw', ...DECLARES.cqww_cw } as never),
    )
    renderPanel()
    await openTab('Contesting')
    const where: Record<string, Element | null> = {
      'Multi-operator class': await screen.findByRole('group', { name: CLASS }),
      'Band category': screen.getByRole('group', { name: BAND }),
      Overlay: screen.getByRole('group', { name: OVERLAY }),
      'Mailing address': fieldLabelled('Mailing address'),
      'Mailing city': fieldLabelled('Mailing city'),
      'Mailing state or province': fieldLabelled('Mailing state or province'),
      'Postal code': fieldLabelled('Postal code'),
      'Mailing country': fieldLabelled('Mailing country'),
      Soapbox: fieldLabelled('Soapbox'),
    }
    const placed = Object.fromEntries(Object.entries(where).map(([name, el]) => [name, groupOf(el)]))
    expect(placed).toEqual(Object.fromEntries(Object.keys(where).map((name) => [name, 'Your entry'])))
  })
})

// Each contest's own answer, read off the panel as the operator picks it: the chips change the
// form, so a pick not yet saved shows its own groups at once.
describe('per contest, as it is picked', () => {
  /** For every contest on the picker, whether `seen` holds once it is picked. */
  async function perContest(seen: () => boolean) {
    renderPanel()
    await openTab('Contesting')
    await screen.findByRole('group', { name: 'Which contest you are entering' })
    const out: string[] = []
    for (const c of CONTESTS) {
      fireEvent.click(chipFor(c.name))
      expect(chipFor(c.name).getAttribute('aria-pressed'), c.id).toBe('true')
      if (seen()) out.push(c.id)
    }
    return out
  }

  it('Field Day\'s group for the two Field Days only, and its power multiplier for ARRL Field Day only', async () => {
    const fdGroup = await perContest(() => legends().includes('Field Day'))
    expect(fdGroup).toEqual(CONTESTS.map((c) => c.id).filter((id) => isFieldDay(id)))
    expect(fdGroup).toEqual(['arrlfd', 'wfd'])
    cleanup()
    const multiplier = await perContest(
      () => screen.queryByRole('group', { name: 'Field Day power multiplier' }) !== null,
    )
    expect(multiplier).toEqual(['arrlfd'])
  })

  it('Power sent only where the exchange sends the station\'s power: none of the shipped contests', async () => {
    const powerSent = await perContest(() => fieldLabelled('Power sent') !== null)
    expect(powerSent).toEqual(CONTESTS.map((c) => c.id).filter((id) => CONTESTS_SENDING_POWER.includes(id)))
    expect(powerSent).toEqual([])
  })

  it('a blank pick is ARRL Field Day: its group and multiplier, and no Power sent', async () => {
    station = { ...station, fdEvent: '' }
    renderPanel()
    await openTab('Contesting')
    await screen.findByRole('group', { name: 'Field Day power multiplier' })
    expect(legends()).toContain('Field Day')
    expect(fieldLabelled('Power sent')).toBeNull()
  })
})

// ⭐ THE ROUND TRIP. A file this release saves, every Contesting value set: each control shows the
// value it holds, under the same name, whichever group it now sits in; and a Save that changed
// nothing writes the file back whole, so no stored choice is lost by a group that is not shown.
describe('a stored settings file, every Contesting value set', () => {
  const ILQP_CLASSES = ['ILLINOIS FIXED HIGH POWER', 'ILLINOIS MOBILE', 'UNLIMITED']

  /** What the tab shows for each stored value, by the control's own name. */
  function shown(): Record<string, string | null> {
    const checked = (name: string | RegExp) =>
      screen.queryByRole('switch', { name })?.getAttribute('aria-checked') ?? null
    const value = (label: string) => boxLabelled(label)?.value ?? null
    const text = (label: string) =>
      (fieldLabelled(label)?.querySelector('textarea') as HTMLTextAreaElement | null)?.value ?? null
    const esmKey = (cockpit: string) =>
      (document.querySelector(
        `[data-esm-cockpit="${cockpit}"] [data-esm-role="again"] select`,
      ) as HTMLSelectElement | null)?.value ?? null
    return {
      contest: pressedIn('Which contest you are entering'),
      // On shows Stop contest…, off shows Start contest… (the live station's state, which the file's
      // fdActive puts the station in).
      'Contest mode': screen.queryByRole('button', { name: 'Stop contest…' })
        ? 'true'
        : screen.queryByRole('button', { name: 'Start contest…' })
          ? 'false'
          : null,
      County: value('County'),
      'State or province': value('State or province'),
      Check: value('Check'),
      'CQ zone': value('CQ zone'),
      'ITU zone': value('ITU zone'),
      'Power sent': value('Power sent'),
      'Entry category': pressedIn('How many operators are at this station'),
      'Power category': pressedIn('Which power category you are entering'),
      'Spotting assistance': pressedIn('Whether your entry uses spotting assistance'),
      'Station type': pressedIn('Whether this is a school club station'),
      'Unassisted entry': checked(/ an unassisted contest entry$/),
      'Email for contest logs': value('Email for contest logs'),
      'Entry class': value('Entry class'),
      Club: value('Club'),
      'Other operators': value('Other operators'),
      'Multi-operator class': screen.queryByRole('group', { name: CLASS }) ? pressedIn(CLASS) : null,
      'Band category': screen.queryByRole('group', { name: BAND }) ? pressedIn(BAND) : null,
      Overlay: screen.queryByRole('group', { name: OVERLAY }) ? pressedIn(OVERLAY) : null,
      'Mailing address': text('Mailing address'),
      'Mailing city': value('Mailing city'),
      'Mailing state or province': value('Mailing state or province'),
      'Postal code': value('Postal code'),
      'Mailing country': value('Mailing country'),
      Soapbox: text('Soapbox'),
      'Super Check Partial': checked('Super Check Partial'),
      'Check for a newer list once a day': checked('Check for a newer list once a day'),
      'Fill the exchange from call history': checked('Fill the exchange from call history'),
      'ESM in the CW cockpit': checked('ESM in the CW cockpit'),
      'ESM in the RTTY cockpit': checked('ESM in the RTTY cockpit'),
      'ESM in the Phone cockpit': checked('ESM in the Phone cockpit'),
      'Call once (S&P)': checked('Call once (S&P)'),
      'RTTY AGN key': esmKey('rtty'),
      'Voice AGN key': esmKey('phone'),
      'FD Class': value('FD Class') ?? value('WFD Class and Category'),
      'ARRL Section': value('ARRL Section'),
      'Power multiplier': screen.queryByRole('group', { name: 'Field Day power multiplier' })
        ? pressedIn('Field Day power multiplier')
        : null,
      'Callsign on the air': value('Callsign on the air'),
      'Position name': value('Position name'),
      'Operator at the key': value('Operator at the key'),
      'Host a club event': checked(/club event hosting$/),
      'Event name': value('Event name'),
      'Host port': value('Host port'),
      'Join event at': value('Join event at'),
      'Spectator scoreboard': checked(/the spectator scoreboard$/),
      'Board port': value('Board port'),
    }
  }

  /** What `storedFile` holds, as each control shows it for `pick`. */
  function stored(pick: string): Record<string, string | null> {
    const fd = isFieldDay(pick)
    const declares = DECLARES[pick] ?? {}
    const address = declares.addressHeaders !== undefined
    return {
      contest: CONTESTS.find((c) => c.id === pick)!.name,
      'Contest mode': 'true',
      County: 'COOK',
      'State or province': 'IL',
      Check: '74',
      'CQ zone': '4',
      'ITU zone': '8',
      // Kept in the file, shown only where an exchange sends it — nowhere in this release.
      'Power sent': CONTESTS_SENDING_POWER.includes(pick) ? 'KW' : null,
      'Entry category': 'MULTI-OP',
      'Power category': 'LOW',
      'Spotting assistance': 'ASSISTED',
      'Station type': 'SCHOOL',
      'Unassisted entry': 'true',
      'Email for contest logs': 'op@example.org',
      'Entry class': pick === 'ilqp' ? 'ILLINOIS MOBILE' : null,
      Club: 'Example Radio Club',
      'Other operators': 'KB9QRS N0AAA',
      // Kept in the file whatever is picked, shown where the contest's rules data offers it.
      'Multi-operator class': declares.transmitterCategories ? 'UNLIMITED' : null,
      'Band category': declares.bandCategories ? '40M' : null,
      Overlay: declares.overlayCategories ? 'ROOKIE' : null,
      'Mailing address': address ? '1 Example Road\nUnit 2' : null,
      'Mailing city': address ? 'Springfield' : null,
      'Mailing state or province': address ? 'IL' : null,
      'Postal code': address ? '62701' : null,
      'Mailing country': address ? 'USA' : null,
      Soapbox: declares.soapbox ? 'A fine weekend.\nFirst licensed 2024.' : null,
      'Super Check Partial': 'false',
      'Check for a newer list once a day': 'false',
      'Fill the exchange from call history': 'false',
      'ESM in the CW cockpit': 'true',
      'ESM in the RTTY cockpit': 'true',
      'ESM in the Phone cockpit': 'true',
      'Call once (S&P)': 'true',
      'RTTY AGN key': 'F5',
      'Voice AGN key': 'F6',
      'FD Class': fd ? '3A' : null,
      'ARRL Section': fd ? 'WI' : null,
      'Power multiplier': pick === 'arrlfd' ? '×5 QRP / battery' : null,
      'Callsign on the air': 'W9XYZ',
      'Position name': 'CW tent',
      'Operator at the key': 'K1ABC',
      'Host a club event': 'false',
      'Event name': 'Example Club Field Day',
      'Host port': '42080',
      'Join event at': '192.168.1.10:42073',
      'Spectator scoreboard': 'true',
      'Board port': '7380',
    }
  }

  it.each(['ilqp', 'arrlfd', 'wfd', 'cqww_cw'])('%s: every control shows its stored value, and Save writes the file back', async (pick) => {
    const file = { ...structuredClone(storedFile), fdEvent: pick } as Record<string, unknown>
    station = structuredClone(file)
    api.get('getFdRuleset').mockImplementation(() =>
      Promise.resolve({
        event: pick,
        entryClasses: pick === 'ilqp' ? ILQP_CLASSES : [],
        ...DECLARES[pick],
      } as never),
    )
    // The file saves Contest mode on, so the station runs its pick.
    renderPanel({ event: pick, log: [] } as unknown as FieldDayStatus)
    await openTab('Contesting')
    await screen.findByRole('group', { name: 'Which contest you are entering' })
    if (pick === 'ilqp') await waitFor(() => expect(boxLabelled('Entry class')).not.toBeNull())
    if (pick === 'cqww_cw') await screen.findByRole('group', { name: CLASS })
    expect(shown()).toEqual(stored(pick))
    // The Station tab's State is the other box, holding the ragchew QTH it was given.
    await openTab('Station')
    expect((await screen.findByDisplayValue('MADISON')).closest('.settings-field')?.textContent).toMatch(/^State/)
    // Save, having changed nothing: the file the station holds afterwards is the file it held.
    fireEvent.click(
      screen.getAllByRole('button', { name: 'Save' }).find((b) => (b as HTMLButtonElement).type === 'submit')!,
    )
    await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
    expect(station).toEqual(file)
  })
})
