// Curating the station's working channel list from a browser: the wire grammar for the four
// gestures the Program view offers, and the grant they ride under. A row is named by its channel
// ID and never by a position, and nothing here can name a path, a project or a list of channels.
import { expect, it } from 'vitest'
import {
  LOG_CAPABILITIES,
  logChange,
  logChangeCapability,
  logChangeCapabilities,
  operationValue,
  type LogChange
} from './operation-protocol'

const REVISION = 'a'.repeat(64)
const change = (edit: unknown): unknown => ({ kind: 'programEdit', revision: REVISION, edit })

it('carries one gesture naming one row by its channel id', () => {
  for (const edit of [
    { action: 'rename', id: 'manual:0', name: 'W1AW' },
    // An empty name is a real choice (the row falls back to its derived name on export).
    { action: 'rename', id: 'manual:0', name: '' },
    { action: 'rename', id: 'manual:0', name: 'a'.repeat(1024) },
    { action: 'remove', id: 'manual:0' },
    { action: 'move', id: 'manual:0', by: -1 },
    { action: 'move', id: 'manual:0', by: 1 },
    { action: 'clear' }
  ])
    expect(logChange(change(edit)), JSON.stringify(edit)).toEqual(change(edit))
})

it('refuses anything wider than those four gestures', () => {
  for (const edit of [
    // A position instead of an id — the whole reason this is keyed.
    { action: 'remove', index: 0 },
    { action: 'rename', index: 0, name: 'W1AW' },
    // A row id or name no `programming` document could have carried.
    { action: 'rename', id: '', name: 'W1AW' },
    { action: 'rename', id: 'a'.repeat(257), name: 'W1AW' },
    { action: 'rename', id: 'manual:0', name: 'a'.repeat(1025) },
    // A newline would plant an extra row in the exported CSV.
    { action: 'rename', id: 'manual:0', name: 'W1AW\nEXTRA,146.940' },
    { action: 'rename', id: 'manual:0', name: 'W1AW\rEXTRA' },
    { action: 'remove', id: '' },
    // A move that is not one place, in either direction.
    { action: 'move', id: 'manual:0', by: 0 },
    { action: 'move', id: 'manual:0', by: 2 },
    { action: 'move', id: 'manual:0', by: -2 },
    { action: 'move', id: 'manual:0', by: '1' },
    // A gesture the view does not offer, or one carrying something it must not.
    { action: 'replace', channels: [] },
    { action: 'import', csv: 'Location,Name' },
    { action: 'clear', path: '/home/op/.config/nexus/radioprog.json' },
    { action: 'clear', project: 'denver-trip' },
    { action: 'remove', id: 'manual:0', index: 0 },
    { action: 'rename', id: 'manual:0' },
    { action: 'move', id: 'manual:0' },
    {}, null, 'clear', []
  ])
    expect(() => logChange(change(edit)), JSON.stringify(edit)).toThrow()
  // The change itself must carry a real document revision, and nothing else.
  for (const bad of [
    { kind: 'programEdit', revision: 'nope', edit: { action: 'clear' } },
    { kind: 'programEdit', edit: { action: 'clear' } },
    { kind: 'programEdit', revision: REVISION },
    { kind: 'programEdit', revision: REVISION, edit: { action: 'clear' }, target: { call: 'W1AW' } }
  ])
    expect(() => logChange(bad), JSON.stringify(bad)).toThrow()
})

it('rides under station control, by its own station hint', () => {
  const edit = change({ action: 'clear' }) as LogChange
  expect(LOG_CAPABILITIES).toContain('programEdit')
  expect(logChangeCapability(edit)).toBe('programEdit')
  expect(logChangeCapabilities(edit)).toEqual(['programEdit'])
})

it('reports a saved list as its own evidence, and a half-written one as unknown', () => {
  const id = crypto.randomUUID()
  for (const value of [
    { operation: 'logChange', operationId: id, outcome: 'applied', evidence: 'programSaved' },
    { operation: 'logChange', operationId: id, outcome: 'rejected', reason: 'contextChanged' },
    { operation: 'logChange', operationId: id, outcome: 'rejected', reason: 'invalidChange' },
    { operation: 'logChange', operationId: id, outcome: 'unknown', reason: 'persistenceUnconfirmed' }
  ])
    expect(operationValue(value)).toEqual(value)
  // POSITIVE CONTROL for that loop: an evidence word nothing produces is still refused, so the
  // four above passed because they are admitted rather than because nothing is checked.
  expect(() => operationValue({ operation: 'logChange', operationId: id, outcome: 'applied', evidence: 'programWritten' })).toThrow()
})
