// Imported ONLY by the explicit preview entry and tests, never by the native adapter.
import fixtures from './fixtures.v2.json'
import type { MonitorSource } from './session'
import { parseFrame } from './protocol'

export type Scenario = keyof typeof fixtures
export const scenarios = Object.keys(fixtures) as Scenario[]

export function fixtureSource(initial: Scenario = 'spe') {
  const controls = { scenario: initial, paused: false }
  let sequence = 0
  let last: unknown = null
  const source: MonitorSource = {
    id: 'explicit-preview', kind: 'fixture',
    read: async (signal) => {
      if (signal.aborted) throw new Error('previewCancelled')
      if (controls.paused && last) return last
      const next = parseFrame(structuredClone(fixtures[controls.scenario]), 'fixture')
      next.sequence = ++sequence
      next.generatedAtMs = Date.now()
      last = next
      return next
    },
  }
  return { controls, source }
}
