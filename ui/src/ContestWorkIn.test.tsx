// @vitest-environment jsdom
//
// "WORK THIS CONTEST IN CW / IN PHONE" (a tester at the Illinois QSO Party, 2026-10-18, looked for Enter
// Sends Message in the Contest window): the Contest screen and its windows never transmit, so they offer
// the route to the cockpit that does, and say so once.
//   · in the main window a button is App's own view switch (`onWorkIn`);
//   · in another window it can only ask: it writes the request the main window hears, and sends NOTHING
//     to the station (the logger window's bridge stays as ContestLogger.txfree.test.tsx reviewed it);
//   · the hint shows until it is dismissed, once for this computer;
//   · neither shows with Contest mode off, nor on a page that only watches.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen, within } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import { ContestView } from './components/ContestView'
import { t } from './i18n'
import { CONTEST_WORK_IN_HINT_KEY, CONTEST_WORK_IN_KEY, workInRequest, workInViewsFor } from './features/contestWorkIn'
import defaultSettings from './components/__fixtures__/defaultSettings.json'
import type { FieldDayStatus } from './types'
import type { FdRulesetDto } from './api'
import { readFileSync } from 'node:fs'
import { resolve } from 'node:path'

/** A contest's banned modes, as the rules seed ships them. */
const SEED = JSON.parse(readFileSync(resolve(process.cwd(), '../crates/tempo-core/src/fd_rules.seed.json'), 'utf8')) as {
  rulesets: { event: string; banned_modes: string[] }[]
}
const bannedIn = (event: string): string[] => {
  const rs = SEED.rulesets.find((r) => r.event === event)
  if (!rs) throw new Error(`${event} is not in the seed`)
  return rs.banned_modes
}
const rulesetDto = (event: string): FdRulesetDto =>
  ({ event, rulesYear: 2026, bannedModes: bannedIn(event), spottingAllowed: true, clusterAllowed: true, enforcement: 'warn' }) as FdRulesetDto

const T0 = 1_792_342_800

const fieldDay = {
  running: false,
  state: 'Idle',
  qsoCount: 1,
  sections: 0,
  points: 0,
  event: 'ilqp',
  role: 'in_state',
  log: [{ call: 'K9AAA', class: '', section: '', band: '20m', mode: 'CW', whenUnix: T0, rcvd: ['599', 'COOK'] }],
  receives: [
    { key: 'RST', kind: 'rst', required: true },
    { key: 'QTH', kind: 'oneOf', required: true, domains: ['il_counties', 'il_mults'] },
  ],
  composing: [
    { key: 'RST', raw: '599' },
    { key: 'QTH', raw: 'KANE', domain: 'il_counties' },
  ],
  composingText: '599 KANE',
  sentExchange: '599 KANE',
  assistanceOn: [],
  upload: { enabled: false, destinations: [], available: [], hint: '' },
  eventStartUnix: T0 - 3600,
  eventEndUnix: T0 + 3600,
}

const snapshot = () => ({
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'fieldDay',
  radio: { dialMhz: 7.03, band: '40m', sideband: 'USB', operatingMode: 'cw', txEnabled: false, txAllowed: true },
  link: { tier: 'FT8' },
  stations: [],
  conversations: [],
  activePeer: null,
  hunt: null,
  fieldDay,
  contestEntry: { rev: 1, call: '', fields: {}, marks: null },
})

let calls: string[] = []
function respond(cmd: string): unknown {
  if (cmd === 'get_snapshot') return snapshot()
  if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: 'ilqp' }
  if (cmd === 'get_window_on_top') return { supported: true, on: false }
  if (cmd === 'contest_removed' || cmd === 'contest_unmerged') return []
  if (/^(get_band_plan|get_need_alerts|get_scp_calls|get_freq_markers)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|resolve_entity|contest_place|get_call_history|get_sat_transponder|contest_logged_qth)$/.test(cmd)) return null
  return {}
}

beforeEach(() => {
  localStorage.clear()
  calls = []
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string): Promise<T> => {
      calls.push(cmd)
      return respond(cmd) as T
    },
  }
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
})
afterEach(() => {
  cleanup()
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

const wait = (ms: number) => act(async () => new Promise((r) => setTimeout(r, ms)))
const settle = () => wait(0)
const group = () => screen.getByRole('group', { name: t('fieldDay.workIn.aria') })
const button = (which: 'cw' | 'phone') =>
  within(group()).getByRole('button', { name: t(which === 'cw' ? 'fieldDay.workIn.cw' : 'fieldDay.workIn.phone') })
const hint = () => screen.queryByRole('note', { name: t('fieldDay.workIn.hint.aria') })
const fd = fieldDay as unknown as FieldDayStatus

describe('in the main window', () => {
  it('each button is the main window’s own switch to that cockpit, and asks no other window', async () => {
    const onWorkIn = vi.fn()
    render(<ContestView fieldDay={fd} fdActive onWorkIn={onWorkIn} />)
    await settle()
    fireEvent.click(button('cw'))
    fireEvent.click(button('phone'))
    expect(onWorkIn.mock.calls).toEqual([['cw'], ['phone']])
    expect(localStorage.getItem(CONTEST_WORK_IN_KEY)).toBeNull()
  })

  it('neither the buttons nor the hint with Contest mode off, nor on a page that only watches', async () => {
    const { unmount } = render(<ContestView fieldDay={null} fdActive={false} onWorkIn={() => {}} />)
    await settle()
    expect(screen.queryByRole('group', { name: t('fieldDay.workIn.aria') })).toBeNull()
    expect(hint()).toBeNull()
    unmount()
    render(<ContestView fieldDay={fd} fdActive observation={defaultSettings as never} />)
    await settle()
    expect(screen.queryByRole('group', { name: t('fieldDay.workIn.aria') })).toBeNull()
    expect(hint()).toBeNull()
  })
})

describe('only the cockpits this station shows', () => {
  it('no CW button with CW turned off in Settings ▸ Features, and none at all with both off', async () => {
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { cw: false, phone: true } }))
    const { unmount } = render(<ContestView fieldDay={fd} fdActive onWorkIn={() => {}} />)
    await settle()
    expect(within(group()).getAllByRole('button').map((b) => b.textContent)).toEqual([t('fieldDay.workIn.phone')])
    unmount()
    localStorage.setItem('nexus.features.v1', JSON.stringify({ profile: 'custom', enabled: { cw: false, phone: false } }))
    render(<ContestView fieldDay={fd} fdActive onWorkIn={() => {}} />)
    await settle()
    expect(screen.queryByRole('group', { name: t('fieldDay.workIn.aria') })).toBeNull()
    expect(hint()).toBeNull()
  })
})

describe('the hint, once', () => {
  it('says sending happens in the cockpit until Got it, then never again on this computer', async () => {
    const { unmount } = render(<ContestView fieldDay={fd} fdActive onWorkIn={() => {}} />)
    await settle()
    expect(hint()?.textContent).toContain(t('fieldDay.workIn.hint'))
    fireEvent.click(within(hint()!).getByRole('button', { name: t('fieldDay.workIn.hint.dismiss') }))
    expect(hint()).toBeNull()
    expect(localStorage.getItem(CONTEST_WORK_IN_HINT_KEY)).toBe('1')
    unmount()
    render(<ContestView fieldDay={fd} fdActive onWorkIn={() => {}} />)
    await settle()
    expect(hint()).toBeNull()
    // The buttons stay: only the hint was dismissed.
    expect(button('cw')).toBeTruthy()
  })
})

describe('in the contest logger window', () => {
  it('a press asks the main window for that cockpit and sends the station nothing', async () => {
    render(<DetachedPanel panel="contestlog" />)
    await wait(400)
    await wait(0)
    const before = calls.length
    fireEvent.click(button('cw'))
    expect(workInRequest(localStorage.getItem(CONTEST_WORK_IN_KEY))).toBe('cw')
    fireEvent.click(button('phone'))
    expect(workInRequest(localStorage.getItem(CONTEST_WORK_IN_KEY))).toBe('phone')
    await settle()
    expect(calls.slice(before).filter((c) => c !== 'get_snapshot' && c !== 'get_settings')).toEqual([])
  })
})

describe('a contest that takes RTTY alone (the ARRL RTTY Roundup)', () => {
  it('offers RTTY, not the CW and Phone it does not allow, and a press is the switch to RTTY', async () => {
    const onWorkIn = vi.fn()
    const rru = { ...fd, event: 'arrlrtty' } as FieldDayStatus
    render(<ContestView fieldDay={rru} fdActive fdRuleset={rulesetDto('arrlrtty')} onWorkIn={onWorkIn} />)
    await settle()
    const buttons = within(group()).getAllByRole('button')
    expect(buttons.map((b) => b.textContent)).toEqual([t('fieldDay.workIn.rtty')])
    expect(buttons[0].getAttribute('title')).toBe(t('fieldDay.workIn.rtty.title'))
    fireEvent.click(buttons[0])
    expect(onWorkIn.mock.calls).toEqual([['rtty']])
  })

  it('CONTROL: a contest that bans FT8 and FT4 still offers CW and Phone, and no RTTY', async () => {
    render(<ContestView fieldDay={fd} fdActive fdRuleset={rulesetDto('ilqp')} onWorkIn={() => {}} />)
    await settle()
    expect(within(group()).getAllByRole('button').map((b) => b.textContent)).toEqual([
      t('fieldDay.workIn.cw'),
      t('fieldDay.workIn.phone'),
    ])
  })

  it('the choice, by the banned list: CW and Phone where allowed, else RTTY where allowed, and only cockpits shown', () => {
    expect(workInViewsFor(bannedIn('arrlrtty'))).toEqual(['rtty'])
    expect(workInViewsFor(bannedIn('wfd'))).toEqual(['cw', 'phone'])
    expect(workInViewsFor([])).toEqual(['cw', 'phone'])
    expect(workInViewsFor(undefined)).toEqual(['cw', 'phone'])
    expect(workInViewsFor(['CW'])).toEqual(['phone'])
    // RTTY switched off in Features: nothing to offer, rather than a banned cockpit.
    expect(workInViewsFor(bannedIn('arrlrtty'), (v) => v !== 'rtty')).toEqual([])
    // CW and Phone switched off in Features is not "banned": RTTY is not offered in their place.
    expect(workInViewsFor([], (v) => v === 'rtty')).toEqual([])
  })
})

describe('a request as stored', () => {
  it('names CW, Phone or RTTY, and nothing else', () => {
    expect(workInRequest(JSON.stringify({ view: 'cw', at: 1 }))).toBe('cw')
    expect(workInRequest(JSON.stringify({ view: 'phone', at: 2 }))).toBe('phone')
    expect(workInRequest(JSON.stringify({ view: 'rtty', at: 4 }))).toBe('rtty')
    expect(workInRequest(JSON.stringify({ view: 'operate', at: 3 }))).toBeNull()
    expect(workInRequest('not json')).toBeNull()
    expect(workInRequest(null)).toBeNull()
  })
})
