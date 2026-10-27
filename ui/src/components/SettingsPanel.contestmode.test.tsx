// @vitest-environment jsdom
//
// CONTEST MODE IN SETTINGS — the one switch that starts whichever contest is picked, offered on
// the Contesting tab and at the top of the Contest screen (`ContestView.modeSwitch.test.tsx`),
// and nowhere else: the copy among the Appearance tab's features is gone.
//
// It was named "Field Day mode" and described as Field Day's ("Turn on for Field Day weekend: the
// Class/Section exchange appears across all modes"), while it already started the Illinois QSO
// Party or CQ WW just the same. So an operator in a QSO party read that the log strip, the
// operator box and the contest keys were Field Day's. Every sentence here is compared whole:
// each names the contest the operator picked, or Field Day only where the line is Field Day's.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent } from '@testing-library/react'
import { SettingsPanel } from './SettingsPanel'
import type { FeaturesApi } from '../useFeatures'
import type { FieldDayStatus } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'

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

/** The station holds these settings, over the shipped defaults. */
const holding = (over: Record<string, unknown>) =>
  api.get('getSettings').mockImplementation(() =>
    Promise.resolve({ ...defaultSettings, mycall: 'W9XYZ', mygrid: 'EN52', ...over } as never),
  )

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
  holding({})
})
afterEach(cleanup)

const openTab = async (name: string) => fireEvent.click(await screen.findByRole('tab', { name }))

/** The switch by its accessible name, and the hint printed beside it. */
/** Contest mode's button: Start contest… with the mode off, Stop contest… with it on. */
const modeButton = async (name: 'Start contest…' | 'Stop contest…') =>
  (await screen.findByRole('button', { name })) as HTMLButtonElement
const hintOf = (sw: HTMLElement) => sw.closest('.settings-field')!.querySelector('.settings-hint')!.textContent

const ON_CONTESTING = (contest: string) =>
  `For ${contest}, the contest picked above. Start contest… walks you through its setup and turns Contest mode on; Stop contest… offers the end-of-contest checklist and turns it off. Both take effect at once. When it is on, the log strips in every mode, the Contest screen and the exports follow its rules.`

describe('Settings ▸ Contesting: the switch is Contest mode, and names the picked contest', () => {
  it('a QSO party: its name on the switch and in the hint, and no Field Day anywhere on it', async () => {
    holding({ fdEvent: 'ilqp' })
    renderPanel()
    await openTab('Contesting')
    const sw = await modeButton('Start contest…')
    const field = sw.closest('.settings-field')!
    expect(field.querySelector('.settings-label')!.textContent).toBe('Contest mode')
    expect(hintOf(sw)).toBe(ON_CONTESTING('Illinois QSO Party'))
    expect(field.textContent).not.toMatch(/Field Day|Class|Section/)
  })

  it('nothing picked yet is ARRL Field Day, the contest a blank pick runs', async () => {
    holding({ fdEvent: '' })
    renderPanel()
    await openTab('Contesting')
    expect(hintOf(await modeButton('Start contest…'))).toBe(ON_CONTESTING('ARRL Field Day'))
  })

  it('on, it says so by name, and offers Stop contest…', async () => {
    holding({ fdEvent: 'cqww_cw', fdActive: true })
    renderPanel({ event: 'cqww_cw', log: [] } as unknown as FieldDayStatus)
    await openTab('Contesting')
    const sw = await modeButton('Stop contest…')
    expect(sw.closest('.settings-field')!.querySelector('[role="status"]')!.textContent).toBe(
      'On: CQ World-Wide DX Contest (CW)',
    )
    // CONTROL: no Start beside it.
    expect(screen.queryByRole('button', { name: 'Start contest…' })).toBeNull()
  })
})

describe('Settings ▸ Appearance ▸ Features: no Contest mode switch, and no Contesting group', () => {
  const groups = () => [...document.querySelectorAll('.settings-featgroup-title')].map((g) => g.textContent)

  it('the switch is on the Contesting tab and the Contest screen only', async () => {
    holding({ fdEvent: 'ilqp' })
    renderPanel()
    await openTab('Appearance')
    expect(groups()).toContain('Core — always on')
    expect(screen.queryByRole('switch', { name: /Contest mode/ })).toBeNull()
    expect(groups()).not.toContain('Contesting')
    // CONTROL: the Contesting tab still has it, named for the picked contest.
    await openTab('Contesting')
    expect(await modeButton('Start contest…')).not.toBeNull()
  })
})

describe('the Class + Section prompt: Field Day’s own line, naming the switch as Contest mode', () => {
  const PROMPT = (event: string) =>
    `Set your Class + Section to start operating. Contest mode is on, but the station won't enter ${event} until both are filled in below.`
  const prompt = () => document.querySelector('#settings-field-day .settings-note')?.textContent ?? null

  it('ARRL Field Day with no class: the event by name', async () => {
    holding({ fdEvent: 'arrlfd', fdActive: true, fdClass: '', fdSection: 'WI' })
    renderPanel()
    await openTab('Contesting')
    // Saved on, but the station has not entered: no session, so Start contest… is offered.
    const sw = await modeButton('Start contest…')
    expect(prompt()).toBe(PROMPT('ARRL Field Day'))
    // Not "Off" beside a prompt that says the mode is on.
    expect(sw.closest('.settings-field')!.querySelector('[role="status"]')!.textContent).toBe('Waiting for Class + Section')
  })

  it('Winter Field Day the same way', async () => {
    holding({ fdEvent: 'wfd', fdActive: true, fdClass: '', fdSection: '' })
    renderPanel()
    await openTab('Contesting')
    // Saved on, but the station has not entered: no session, so Start contest… is offered.
    await modeButton('Start contest…')
    expect(prompt()).toBe(PROMPT('Winter Field Day'))
  })

  it('CONTROL: a QSO party sends no class or section, so it never asks for them', async () => {
    holding({ fdEvent: 'ilqp', fdActive: true, fdClass: '', fdSection: '' })
    renderPanel()
    await openTab('Contesting')
    // Saved on, but the station has not entered: no session, so Start contest… is offered.
    const sw = await modeButton('Start contest…')
    expect(prompt()).toBeNull()
    expect(sw.closest('.settings-field')!.querySelector('[role="status"]')!.textContent).toBe('Off')
  })
})

describe('the switch sits in Contest, under the picker, and says nothing of Field Day for another contest', () => {
  /** Every word a block shows, and every name and tooltip it gives a control. */
  const saidIn = (el: Element) => [
    el.textContent ?? '',
    ...[...el.querySelectorAll('[aria-label], [title]')].flatMap((c) =>
      [c.getAttribute('aria-label'), c.getAttribute('title')].filter((v): v is string => !!v),
    ),
  ]
  /** The switch's own row and the rules data's, which moved with it. */
  const rows = (sw: HTMLElement) => [
    sw.closest('.settings-field')!,
    screen.getByRole('button', { name: 'Check for rules updates' }).closest('.settings-field')!,
  ]

  it('a QSO party: in Contest, with its rules data, and no Field Day in either row', async () => {
    holding({ fdEvent: 'ilqp' })
    renderPanel()
    await openTab('Contesting')
    const sw = await modeButton('Start contest…')
    expect(sw.closest('fieldset')!.querySelector('legend')!.textContent).toBe('Contest')
    expect(rows(sw).flatMap(saidIn).filter((s) => /Field Day/.test(s))).toEqual([])
    expect(rows(sw)[1].textContent).toContain(
      'Scoring, dates, exchanges and multipliers for every contest in the Contest list above. Checked on demand; a downloaded update applies at the next launch.',
    )
    // …and Field Day's own group is not on the tab at all.
    expect(document.querySelector('#settings-field-day')).toBeNull()
  })

  it('CONTROL: on ARRL Field Day the same scan finds Field Day in the switch’s hint, and its power multiplier in its own group', async () => {
    holding({ fdEvent: 'arrlfd' })
    renderPanel()
    await openTab('Contesting')
    const sw = await modeButton('Start contest…')
    expect(rows(sw).flatMap(saidIn).filter((s) => s.includes(ON_CONTESTING('ARRL Field Day')))).toHaveLength(1)
    const fd = document.querySelector('#settings-field-day')!
    expect(fd.querySelector('legend')!.textContent).toBe('Field Day')
    expect(saidIn(fd)).toContain('Field Day power multiplier')
  })
})
