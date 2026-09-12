import { expect, it } from 'vitest'
import { parseMemories } from './memories'
import { memoryBank } from '../remote-native/memoryBank'
import type { QueryPage } from './application-query-protocol'
import fixture from './__fixtures__/memories.json'
const page = (): QueryPage => ({ type: 'applicationPage', requestId: crypto.randomUUID(), snapshotId: crypto.randomUUID(),
  collection: 'memories', offset: 0, total: 0, retained: 0, nextCursor: null, ageMs: 0, rows: [],
  meta: { capturedAgeMs: 12, source: { bank: structuredClone(fixture), sourceAgeMs: 250 } } as unknown as QueryPage['meta'] })
it('preserves every channel/group field, including free modes, odd split, tone, net and favorite order', () => {
  expect(parseMemories(page())).toEqual({ bank: fixture, sourceAgeMs: 250, capturedAgeMs: 12 })
  expect(memoryBank({ version: 2, memories: [], groups: [] })).toBe(true)
})
it('refuses expired, incomplete, foreign and oversized banks instead of repairing or persisting them', () => {
  for (const patch of [{ collection: 'log' }, { offset: 1 }, { rows: [{}] }, { retained: 1 }, { total: 1 }, { nextCursor: crypto.randomUUID() }]) {
    expect(() => parseMemories({ ...page(), ...patch } as QueryPage)).toThrow('invalidMemories')
  }
  for (const patch of [{ sourceAgeMs: 60_000 }, { sourceAgeMs: -1 }, { sourceAgeMs: 0.5 }, { sourceAgeMs: 0, extra: 'no' }, { bank: null }]) {
    const p = page(); p.meta = { capturedAgeMs: 0, source: { bank: fixture, sourceAgeMs: 0, ...patch } } as unknown as QueryPage['meta']
    expect(() => parseMemories(p)).toThrow('invalidMemories')
  }
  for (const patch of [{ version: 1 }, { extra: 'secret' }, { memories: [{}] }, { memories: Array(513).fill(fixture.memories[0]) }, { groups: Array(65).fill(fixture.groups[0]) }]) {
    expect(memoryBank({ ...fixture, ...patch })).toBe(false)
  }
  const duplicate = structuredClone(fixture); duplicate.memories[1].id = duplicate.memories[0].id
  expect(memoryBank(duplicate)).toBe(false)
  const unknown = structuredClone(fixture) as Record<string, unknown>
  unknown.memories = [{ ...fixture.memories[0], vaultKey: 'must not cross' }]
  expect(memoryBank(unknown)).toBe(false)
  expect(memoryBank({ ...fixture, memories: [{ ...fixture.memories[0], notes: 'é'.repeat(513) }] })).toBe(false)
})

it('accepts the complete count limit and refuses otherwise valid banks over the count or byte limit', () => {
  const channel = { id: 'm', name: 'Calling', kind: 'calling', rxMhz: 14.06, mode: 'CW', groups: [], favorite: false, source: 'user' }
  const rows = Array.from({ length: 513 }, (_, i) => ({ ...channel, id: `m${i}` }))
  expect(memoryBank({ version: 2, groups: [], memories: rows.slice(0, 512) })).toBe(true)
  expect(memoryBank({ version: 2, groups: [], memories: rows })).toBe(false)
  const rich = rows.slice(0, 128).map(row => ({ ...row, notes: 'x'.repeat(1024) }))
  expect(memoryBank({ version: 2, groups: [], memories: rich.slice(0, 64) })).toBe(true)
  expect(memoryBank({ version: 2, groups: [], memories: rich })).toBe(false)
})
