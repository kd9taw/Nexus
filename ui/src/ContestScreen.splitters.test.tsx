// @vitest-environment jsdom
//
// THE CONTEST SCREEN'S DIVIDERS (the Illinois QSO Party, 2026-10-18: "adjustable/draggable splitters to
// resize sub-windows (county list etc.)"). Two, each a PaneSeam strip divider sized per window:
//   · between the multiplier board (the county list, which grows) and the contest log under it: it sizes
//     the log, whose stock height is the sheet's 190 px cap, and the board takes what is left;
//   · between the last contacts and the screen under them, where a log line tops the screen (the main
//     window's Contest screen and the logger window): it sizes the last contacts.
// Both are mounted here in the logger window, which carries both. jsdom lays nothing out, so the boxes the
// dividers measure are stubbed: the screen 600 px tall, the window 900 px.
import { describe, it, expect, beforeEach, afterEach } from 'vitest'
import { render, cleanup, act, fireEvent, screen } from '@testing-library/react'
import { DetachedPanel } from './DetachedPanel'
import { t } from './i18n'
import defaultSettings from './components/__fixtures__/defaultSettings.json'

const T0 = 1_792_342_800

const snapshot = () => ({
  mycall: 'KD9TAW',
  mygrid: 'EN52',
  mode: 'FieldDay',
  radio: { dialMhz: 14.25, band: '20m', sideband: 'USB', operatingMode: 'phone', txEnabled: false, txAllowed: true },
  link: { tier: 'FT8' },
  stations: [],
  conversations: [],
  activePeer: null,
  hunt: null,
  fieldDay: {
    running: false,
    state: 'Idle',
    qsoCount: 1,
    sections: 0,
    points: 0,
    event: 'arrlfd',
    log: [{ call: 'K9AAA', class: '3A', section: 'IL', band: '20m', mode: 'PH', submode: 'SSB', whenUnix: T0 }],
    receives: [
      { key: 'CLASS', kind: 'fdClass', required: true },
      { key: 'SECTION', kind: 'oneOf', required: true, domains: ['fd_sections'] },
    ],
    composing: [
      { key: 'CLASS', raw: '3A' },
      { key: 'SECTION', raw: 'IL', domain: 'fd_sections' },
    ],
    composingText: '3A IL',
    sentExchange: '3A IL',
    assistanceOn: [],
    upload: { enabled: false, destinations: [], available: [], hint: '' },
    eventStartUnix: T0 - 3600,
    eventEndUnix: T0 + 3600,
  },
  contestEntry: { rev: 1, call: '', fields: {}, marks: null },
})

function respond(cmd: string): unknown {
  if (cmd === 'get_snapshot') return snapshot()
  if (cmd === 'get_settings') return { ...defaultSettings, fdActive: true, fdEvent: 'arrlfd' }
  if (cmd === 'get_window_on_top') return { supported: true, on: false }
  if (cmd === 'contest_removed' || cmd === 'contest_unmerged') return []
  if (/^(get_band_plan|get_need_alerts|get_scp_calls)$/.test(cmd)) return []
  if (/^(get_propagation|get_fd_ruleset|resolve_entity|contest_place|get_call_history|get_sat_transponder)$/.test(cmd)) return null
  return {}
}

const LOG_KEY = 'nexus.split.contest.log'
const RECENT_KEY = 'nexus.split.contest.recent'
const realRect = HTMLElement.prototype.getBoundingClientRect

beforeEach(() => {
  localStorage.clear()
  window.__TAURI_INTERNALS__ = {
    invoke: async <T,>(cmd: string): Promise<T> => respond(cmd) as T,
  }
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  } as unknown as typeof ResizeObserver
  // The two boxes the dividers measure, in CSS px at no zoom (the window's own Text size sets one);
  // everything else lays out as jsdom does (nothing).
  Object.defineProperty(HTMLElement.prototype, 'currentCSSZoom', { value: 1, configurable: true })
  HTMLElement.prototype.getBoundingClientRect = function (this: HTMLElement) {
    const height = this.matches('.panel.fieldday') ? 600 : this.matches('.contest-logger') ? 900 : 0
    return { top: 0, left: 0, width: 1024, height, right: 1024, bottom: height, x: 0, y: 0, toJSON: () => ({}) } as DOMRect
  }
})
afterEach(() => {
  cleanup()
  HTMLElement.prototype.getBoundingClientRect = realRect
  delete (HTMLElement.prototype as { currentCSSZoom?: number }).currentCSSZoom
  delete (window as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__
})

const wait = (ms: number) => act(async () => new Promise((r) => setTimeout(r, ms)))
async function mountLogger(): Promise<void> {
  render(<DetachedPanel panel="contestlog" />)
  await wait(400)
  await wait(0)
}
const fdScreen = () => document.querySelector('.panel.fieldday') as HTMLElement
const fdLog = () => document.querySelector('.panel.fieldday > .fd-log') as HTMLElement
const logSeam = () => screen.getByRole('separator', { name: t('fieldDay.log.splitter.label') })
const recentSeam = () => screen.getByRole('separator', { name: t('contestLogger.recent.splitter.label') })

describe('the divider between the multiplier board and the contest log', () => {
  it('sits right above the log, and leaves the log its stock height until it is moved', async () => {
    await mountLogger()
    expect(logSeam().nextElementSibling).toBe(fdLog())
    expect(fdLog().hasAttribute('data-sized')).toBe(false)
    expect(fdScreen().style.getPropertyValue('--fd-log-h')).toBe('')
  })

  it('clamps a stored height to THIS window on load, and keeps the preference as it was stored', async () => {
    localStorage.setItem(LOG_KEY, '90')
    await mountLogger()
    // 90 % of the screen is more than the log may take (three quarters of it): 450 of 600 px.
    expect(fdScreen().style.getPropertyValue('--fd-log-h')).toBe('75%')
    expect(fdLog().hasAttribute('data-sized')).toBe(true)
    expect(logSeam().getAttribute('aria-valuenow')).toBe('450')
    expect(localStorage.getItem(LOG_KEY)).toBe('90')
  })

  it('moves by keyboard: the divider is above the log, so Up grows it', async () => {
    localStorage.setItem(LOG_KEY, '40')
    await mountLogger()
    expect(logSeam().getAttribute('aria-valuenow')).toBe('240')
    fireEvent.keyDown(logSeam(), { key: 'ArrowUp' })
    expect(logSeam().getAttribute('aria-valuenow')).toBe('256')
    expect(Number(localStorage.getItem(LOG_KEY))).toBeCloseTo((256 / 600) * 100, 6)
    fireEvent.keyDown(logSeam(), { key: 'ArrowDown', shiftKey: true })
    expect(logSeam().getAttribute('aria-valuenow')).toBe('192')
  })

  it('a double-click gives the log its stock height back', async () => {
    localStorage.setItem(LOG_KEY, '40')
    await mountLogger()
    fireEvent.doubleClick(logSeam())
    expect(fdLog().hasAttribute('data-sized')).toBe(false)
    expect(fdScreen().style.getPropertyValue('--fd-log-h')).toBe('')
    expect(localStorage.getItem(LOG_KEY)).toBe('')
  })
})

describe('the divider between the last contacts and the screen under them', () => {
  const recent = () => document.querySelector('.contest-logger > .contest-logger-recent') as HTMLElement
  const host = () => document.querySelector('.contest-logger') as HTMLElement

  it('sits right under the last contacts, which keep their own height until it is moved', async () => {
    await mountLogger()
    expect(recentSeam().previousElementSibling).toBe(recent())
    expect(recent().hasAttribute('data-sized')).toBe(false)
    expect(host().style.getPropertyValue('--contest-recent-h')).toBe('')
  })

  it('clamps a stored height to THIS window on load, moves by keyboard, and resets on a double-click', async () => {
    localStorage.setItem(RECENT_KEY, '95')
    await mountLogger()
    // At most 90 % of the window: 810 of 900 px.
    expect(host().style.getPropertyValue('--contest-recent-h')).toBe('90%')
    expect(recent().hasAttribute('data-sized')).toBe(true)
    expect(localStorage.getItem(RECENT_KEY)).toBe('95')
    // Below the list, so Up shrinks it.
    fireEvent.keyDown(recentSeam(), { key: 'ArrowUp' })
    expect(recentSeam().getAttribute('aria-valuenow')).toBe('794')
    fireEvent.doubleClick(recentSeam())
    expect(recent().hasAttribute('data-sized')).toBe(false)
    expect(host().style.getPropertyValue('--contest-recent-h')).toBe('')
  })

  it('is not drawn with the entry alone, where there are no last contacts', async () => {
    localStorage.setItem('nexus.contestLogger.layout', 'entry')
    await mountLogger()
    expect(screen.queryByRole('separator', { name: t('contestLogger.recent.splitter.label') })).toBeNull()
  })
})
