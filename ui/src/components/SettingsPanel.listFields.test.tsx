// @vitest-environment jsdom
//
// #370 — SIX SETTINGS LIST FIELDS ATE THEIR SEPARATOR AS IT WAS TYPED. Each box showed its list
// joined with ", " and re-parsed its text on every keystroke, and React wrote the parsed list
// straight back into the box. So the comma a parse drops could never be typed ("W1ABC," came back
// as "W1ABC" and the next call glued on: "W1ABCK2DEF"), and neither could a space in the
// quick-reply chips, which trim each entry ("TNX QSO" came back as "TNXQSO"). A pasted list always
// worked, because it arrives in one change. The six, all on Settings ▸ Digital: APRS-IS Watched
// calls, the digipeater path, JS8 Groups, and the Chat, QSO and Band / CQ quick-reply chips.
//
// Each box now keeps the operator's own text while they type and parses it on EVERY change, so a
// Save that never leaves the box still saves what is in it. (The Blocked-callsigns box, whose
// raw-text pattern this follows, saves only as it is left; that would be the trap here, because
// Enter in any of these boxes submits the form.) Each parse is the one that was there.
//
// The typing is ONE KEYSTROKE AT A TIME (`typeInto`): each character goes onto the end of the box's
// CURRENT text, what the DOM holds after React has answered the last one. A whole-string change is a
// paste, and a paste never showed the bug.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within, waitFor } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { Settings } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'

// THE BUDGET (2026-10-09). The slowest case here, "'APRS-IS Watched calls': typed one key at a time, the…", takes
// 0.38 s and 0.40 s on one core (two runs); a loaded full suite on this box has run cases up to 20 times slower than
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

// Every export of `../api`, derived from the real module (the SettingsPanel.js8 pattern), so a verb
// missing from a hand-kept list cannot throw on mount.
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

/** The fixture with a callsign (a Save refuses without one), the APRS-IS feed on (Watched calls is
 *  greyed out while it is off) and no JS8 groups yet. */
const SETTINGS = { ...defaultSettings, mycall: 'KD9TAW', mygrid: 'EN52', aprsIsEnabled: true, js8Groups: [] as string[] }

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
  api.get('appVersion').mockImplementation(() => Promise.resolve('0.21.2'))
  api.get('getSettings').mockImplementation(() => Promise.resolve(SETTINGS as never))
})
afterEach(cleanup)

async function openDigital() {
  render(
    <SettingsPanel
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
  fireEvent.click(await screen.findByRole('tab', { name: 'Digital' }))
  await screen.findByText('Transmit speed') // the tab has rendered once a JS8 control is up
}

interface Field {
  name: string
  /** The fieldset the box sits in: two of the labels ("Chat", "QSO") are common words. */
  fieldset: string
  label: string
  typed: string
  expected: string[]
  saved: (s: Settings) => unknown
}

const FIELDS: Field[] = [
  { name: 'APRS-IS Watched calls', fieldset: 'settings-aprs', label: 'Watched calls', typed: 'W1ABC,K2DEF', expected: ['W1ABC', 'K2DEF'], saved: (s) => s.aprsIsWatchCalls },
  { name: 'the digipeater path', fieldset: 'settings-aprs', label: 'Digipeater path', typed: 'WIDE1-1,WIDE2-2', expected: ['WIDE1-1', 'WIDE2-2'], saved: (s) => s.aprsPath },
  { name: 'JS8 Groups', fieldset: 'settings-js8', label: 'Groups', typed: '@ARES,@SKCC', expected: ['@ARES', '@SKCC'], saved: (s) => s.js8Groups },
  { name: 'the Chat chips', fieldset: 'settings-quick-reply-macros', label: 'Chat', typed: 'TNX QSO,73', expected: ['TNX QSO', '73'], saved: (s) => s.macros.chat },
  { name: 'the QSO chips', fieldset: 'settings-quick-reply-macros', label: 'QSO', typed: 'TNX QSO,RR73', expected: ['TNX QSO', 'RR73'], saved: (s) => s.macros.qso },
  { name: 'the Band / CQ chips', fieldset: 'settings-quick-reply-macros', label: 'Band / CQ', typed: 'CQ POTA,73 to all', expected: ['CQ POTA', '73 to all'], saved: (s) => s.macros.band },
]
const byName = (name: string) => FIELDS.find((f) => f.name === name)!

/** The text box a field's label names, inside that field's own fieldset. */
function box(f: Field): HTMLInputElement {
  const fs = document.getElementById(f.fieldset)
  expect(fs, `no <fieldset id="${f.fieldset}">`).not.toBeNull()
  const span = within(fs!)
    .getAllByText(f.label)
    .find((n) => n.tagName === 'SPAN' && n.classList.contains('settings-label'))
  expect(span, `no settings-label span "${f.label}"`).toBeDefined()
  const input = span!.closest('label, .settings-field')!.querySelector('input')
  expect(input, `no input in the "${f.label}" field`).not.toBeNull()
  return input as HTMLInputElement
}

/** One keystroke at a time: each character onto the end of the box's CURRENT text, as one change. */
function typeInto(input: HTMLInputElement, text: string) {
  for (const ch of text) fireEvent.change(input, { target: { value: input.value + ch } })
}

/** Select-all and delete: one change, to nothing. */
const clear = (input: HTMLInputElement) => fireEvent.change(input, { target: { value: '' } })

/** Enter in the box, as a browser handles it: implicit submission clicks the form's DEFAULT button
 *  (its first submit button in tree order) and focus never leaves the box. jsdom performs no
 *  implicit submission of its own, so this does its two steps, and checks the default button is the
 *  panel's Save. Returns what the Save sent. */
async function pressEnter(input: HTMLInputElement): Promise<Settings> {
  expect(fireEvent.keyDown(input, { key: 'Enter', code: 'Enter' }), 'nothing cancels the Enter').toBe(true)
  const byDefault = [...input.form!.elements].find(
    (el): el is HTMLButtonElement => el instanceof HTMLButtonElement && el.type === 'submit',
  )
  expect(byDefault?.textContent, 'the form’s default button is its Save').toBe('Save')
  fireEvent.click(byDefault!)
  expect(document.activeElement, 'the box was never left').toBe(input)
  await waitFor(() => expect(api.get('setSettings')).toHaveBeenCalled())
  const calls = api.get('setSettings').mock.calls
  return calls[calls.length - 1][0] as Settings
}

describe('#370 the six list fields take a typed separator', () => {
  it.each(FIELDS)('$name: typed one key at a time, the separator stays in the box', async (f) => {
    await openDigital()
    const input = box(f)
    input.focus()
    clear(input)
    typeInto(input, f.typed)
    expect(input.value, 'the box shows what was typed').toBe(f.typed)
  })

  it.each(FIELDS)('$name: Enter, without leaving the box, saves the list that was typed', async (f) => {
    await openDigital()
    const input = box(f)
    input.focus()
    clear(input)
    typeInto(input, f.typed)
    expect(f.saved(await pressEnter(input))).toEqual(f.expected)
  })
})

describe('#370 each field parses as it did', () => {
  it('Watched calls and the path are upper-cased and trimmed; typing in lower case saves the same', async () => {
    await openDigital()
    const calls = box(byName('APRS-IS Watched calls'))
    calls.focus()
    typeInto(calls, 'w1abc, k2def-9 ,')
    expect(calls.value, 'the box keeps the operator’s own text while they type').toBe('w1abc, k2def-9 ,')
    const path = box(byName('the digipeater path'))
    path.focus()
    clear(path)
    typeInto(path, 'wide1-1, wide2-1')
    const sent = await pressEnter(path)
    expect(sent.aprsIsWatchCalls).toEqual(['W1ABC', 'K2DEF-9'])
    expect(sent.aprsPath).toEqual(['WIDE1-1', 'WIDE2-1'])
  })

  it('JS8 Groups are upper-cased and take one leading @, typed with or without it', async () => {
    await openDigital()
    const groups = box(byName('JS8 Groups'))
    groups.focus()
    typeInto(groups, 'ares, @@skcc ,,')
    expect((await pressEnter(groups)).js8Groups).toEqual(['@ARES', '@SKCC'])
  })

  it('the chips are trimmed and nothing else: never upper-cased', async () => {
    await openDigital()
    const chat = box(byName('the Chat chips'))
    chat.focus()
    clear(chat)
    typeInto(chat, '  tnx qso , 73 ,')
    expect((await pressEnter(chat)).macros.chat).toEqual(['tnx qso', '73'])
  })

  it('an emptied digipeater path is saved empty, which means direct, no digipeaters', async () => {
    await openDigital()
    const path = box(byName('the digipeater path'))
    expect(path.value, 'the stored path shows first').toBe('WIDE1-1, WIDE2-1')
    path.focus()
    clear(path)
    expect((await pressEnter(path)).aprsPath).toEqual([])
  })

  it('a pasted list, one change, still saves as it always did', async () => {
    await openDigital()
    const calls = box(byName('APRS-IS Watched calls'))
    calls.focus()
    fireEvent.change(calls, { target: { value: 'W1ABC, K2DEF' } })
    expect((await pressEnter(calls)).aprsIsWatchCalls).toEqual(['W1ABC', 'K2DEF'])
  })
})

describe('#370 leaving the box shows the list as it was read', () => {
  it('the typed text gives way to the parsed list, joined the one way', async () => {
    await openDigital()
    const groups = box(byName('JS8 Groups'))
    groups.focus()
    typeInto(groups, 'ares,skcc')
    fireEvent.blur(groups)
    expect(groups.value).toBe('@ARES, @SKCC')
    const chat = box(byName('the Chat chips'))
    chat.focus()
    clear(chat)
    typeInto(chat, 'TNX QSO ,73,')
    fireEvent.blur(chat)
    expect(chat.value).toBe('TNX QSO, 73')
  })
})
