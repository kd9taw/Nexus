// ONE DECISION, SHOWN AS IT WAS MADE: which decodes are worth interrupting the operator for.
//
// The station decides (`NeededAlerts`, crates/tempo-app/src/needed_alert.rs) and raises each alert
// as the `alert` event; this screen only shows it (`showRaisedAlert` → `showNeededAlert`). Both
// halves are held to one table of cases that lives with the Rust crate
// (crates/tempo-app/tests/fixtures/needed-alerts.json): runs of decode batches, each with the
// settings, QSO, watch list, dial and geographic scope it arrives under, and the alerts it raises —
// which toast, its words' parts, how long, how loud, and every tone in the order it sounds. The
// expected values were captured from the screen's own decision before it moved to the station,
// together with every case its tests held it to.
//
// End to end, decodes in → alerts shown: the station's test walks each step's decodes through
// the decision and must raise exactly the step's alerts, as the event carries them (`kind`,
// `call`, `entity`, `grid`, `tier`, `watch`); this test raises those, step by step, to the
// window as the event, and the window must show exactly the step's toasts and sound its tones —
// each alert once.

import { describe, expect, it, vi, beforeEach, afterEach } from 'vitest'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import { dirname, resolve } from 'node:path'
import type { NeededAlert, QsoContext } from './alerts'
import { __resetStationAlertsForTest, showRaisedAlert } from './useStationAlerts'
import { pushToast } from './toast'
import type { WatchFilter } from './watchlist'

vi.mock('./toast', () => ({ pushToast: vi.fn() }))
// Every word as its catalog key and parts, so a case reads what the toast is made of, not English.
vi.mock('./i18n', async (actual) => ({
  ...(await actual<typeof import('./i18n')>()),
  t: (key: string, parts?: Record<string, unknown>) => JSON.stringify(parts ? { key, parts } : { key }),
}))
const toasts = vi.mocked(pushToast)

// Every tone the alerts sound, in the order they sound.
const tones: number[] = []
class Tones {
  state = 'running'
  currentTime = 0
  destination = {}
  createOscillator() {
    const osc = {
      type: 'sine',
      frequency: { value: 0, setValueAtTime() {}, exponentialRampToValueAtTime() {} },
      connect() {},
      start: () => tones.push(osc.frequency.value),
      stop() {},
    }
    return osc
  }
  createGain() {
    return { gain: { setValueAtTime() {}, exponentialRampToValueAtTime() {} }, connect() {} }
  }
}
vi.stubGlobal('window', {
  setTimeout: (run: () => void, ms: number) => globalThis.setTimeout(run, ms),
  AudioContext: Tones,
} as unknown as Window & typeof globalThis)

interface Step {
  atMs: number
  settings?: Record<string, unknown>
  qso?: QsoContext
  watchlist?: WatchFilter[]
  dialMhz?: number
  geoScope?: string[]
  decodes: Record<string, unknown>[]
  alerts: Record<string, unknown>[]
  sounds: number[]
}
interface Table {
  baseDecode: Record<string, unknown>
  baseSettings: Record<string, unknown>
  scenarios: { name: string; steps: Step[] }[]
  pota: { name: string; ticks: { refs: string[]; sounds: number[] }[] }[]
}

const table = JSON.parse(
  readFileSync(
    resolve(dirname(fileURLToPath(import.meta.url)), '../../crates/tempo-app/tests/fixtures/needed-alerts.json'),
    'utf8',
  ),
) as Table

const KINDS: Record<string, string> = {
  'alerts.watch': 'watch',
  'alerts.newDxcc': 'newDxcc',
  'alerts.rareGrid': 'rareGrid',
  'alerts.newGrid': 'newGrid',
  'alerts.myCall': 'myCall',
  'alerts.cq': 'cq',
}

function keyOf(text: unknown): string | null {
  return typeof text === 'string' && text.startsWith('{') ? (JSON.parse(text) as { key: string }).key : null
}

/** One toast, as the table writes an alert. */
function alertOf(call: unknown[]): Record<string, unknown> {
  const [text, tone, ms, options] = call as [string, string, number, { prominent?: boolean; actionLabel: string }]
  const { key, parts } = JSON.parse(text) as { key: string; parts: Record<string, string> }
  const where = parts.where ? (JSON.parse(parts.where) as { parts: { entity: string } }).parts.entity : null
  const tier = keyOf(parts.tier)
  return {
    kind: KINDS[key] ?? key,
    call: keyOf(parts.call) === 'alerts.station' ? null : parts.call,
    entity: where,
    grid: parts.grid ? parts.grid.trim() : null,
    tier: tier ? tier.replace('alerts.tier.', '') : null,
    watch: parts.what ?? null,
    tone,
    ms,
    prominent: options.prominent ?? false,
    action: keyOf(options.actionLabel)?.replace('alerts.action.', ''),
  }
}

/** An alert as the station raises it: the table's alert, its presentation left to the screen. */
function raisedOf(alert: Record<string, unknown>): NeededAlert {
  const { kind, call, entity, grid, tier, watch } = alert as unknown as NeededAlert
  const work = call ? { message: `CQ ${call}`, snr: -10, freqHz: 1500, tier: 'FT8' as const } : null
  return { kind, call, entity, grid, tier, watch, work }
}

let nextId = 1
/** Raise one alert to the window, as the event carries it. */
function raise(kind: string, alert: unknown, id = nextId++): void {
  showRaisedAlert({ v: 1, id, kind, atMs: 0, alert }, () => {})
}

/** The table's alerts raised to the window, step by step: each step's toasts and tones, and each
 *  POTA tick's tones (the station raises one `parks` alert for a tick that found a new park). */
function observe(): Table {
  __resetStationAlertsForTest()
  const scenarios = table.scenarios.map((scenario) => {
    const steps = scenario.steps.map((step) => {
      toasts.mockClear()
      tones.length = 0
      for (const alert of step.alerts) raise('needed', raisedOf(alert))
      vi.runAllTimers()
      return { ...step, alerts: toasts.mock.calls.map(alertOf), sounds: [...tones] }
    })
    return { ...scenario, steps }
  })
  const pota = table.pota.map((run) => {
    const ticks = run.ticks.map((tick) => {
      tones.length = 0
      if (tick.sounds.length > 0) raise('parks', { references: tick.refs })
      vi.runAllTimers()
      return { ...tick, sounds: [...tones] }
    })
    return { ...run, ticks }
  })
  return { ...table, scenarios, pota }
}

beforeEach(() => {
  vi.useFakeTimers()
})
afterEach(() => {
  vi.useRealTimers()
})

describe('the screen shows exactly the needed-station alerts the station raises', () => {
  it('the shared table is there, and raises every kind', () => {
    const kinds = new Set(table.scenarios.flatMap((s) => s.steps.flatMap((st) => st.alerts.map((a) => a.kind))))
    expect([...kinds].sort()).toEqual(['cq', 'myCall', 'newDxcc', 'newGrid', 'rareGrid', 'watch'])
    // Silence occurs too: a table of alerts alone could not tell a decider from one that raises
    // everything.
    expect(table.scenarios.some((s) => s.steps.some((st) => st.alerts.length === 0))).toBe(true)
    expect(table.pota.some((p) => p.ticks.some((t) => t.sounds.length === 0))).toBe(true)
  })

  it('the desktop shows every step of the table as the table writes it', () => {
    const seen = observe()
    for (const [i, scenario] of table.scenarios.entries())
      for (const [j, step] of scenario.steps.entries())
        expect(
          { alerts: seen.scenarios[i].steps[j].alerts, sounds: seen.scenarios[i].steps[j].sounds },
          `${scenario.name}, step ${j + 1}`,
        ).toEqual({ alerts: step.alerts, sounds: step.sounds })
    for (const [i, run] of table.pota.entries())
      for (const [j, tick] of run.ticks.entries())
        expect(seen.pota[i].ticks[j].sounds, `${run.name}, tick ${j + 1}`).toEqual(tick.sounds)
  })
})
