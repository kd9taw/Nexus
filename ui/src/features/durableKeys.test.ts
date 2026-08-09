// The durable-key CLASSIFICATION (#28), checked against the per-surface classification of the
// same keys. Deliberately a NODE test that imports the scanner's list: the two lists answer
// different questions about the same keys and must never both claim one.
//
// Separate file from durableStore.test.ts because that one runs in jsdom, and importing a test
// module re-executes its suite in the importer's environment — the scanner's cases are node
// tests and would run twice, in the wrong environment.
import { describe, it, expect } from 'vitest'
import { PER_SURFACE } from '../storage-scope.test'
import { DURABLE_KEYS, isDurable } from './durableStore'

describe('what counts as operator data', () => {
  // The whole point of #28 is that these two classifications answer different questions about
  // the same keys — "does this survive a reinstall" and "does each window get its own". A key in
  // both would be incoherent: a per-surface key has no single value to make durable. This caught
  // `nexus-ui-scale-mode` while the list was being written.
  it('no key is both durable and per-surface', () => {
    const both = DURABLE_KEYS.filter((k) => (PER_SURFACE as string[]).includes(k))
    expect(both, `a per-surface key has no single durable value: ${both.join(', ')}`).toEqual([])
  })

  it('covers the operator data the audit named, and no cosmetics', () => {
    for (const k of [
      'nexus.memory.bank.v2',
      'nexus.watchlist',
      'nexus.sats.chasing',
      'nexus.dxped.chasing',
      'nexus.sats.alarms',
      'nexus.profiles',
    ]) {
      expect(isDurable(k), `${k} holds operator data and must be durable`).toBe(true)
    }
    // The other half of the classification: these are preference, and belong in localStorage.
    for (const k of ['nexus.awardsTab', 'nexus-density', 'nexus.connect.map3d', 'nexus.dev.xray']) {
      expect(isDurable(k), `${k} is cosmetic — durability buys nothing`).toBe(false)
    }
  })

  it('carries the orphaned memory-bank v1 key', () => {
    // A schema change was once handled by minting `.v2` and orphaning `.v1`. An operator who
    // never launched a version that migrated still has their channels only under v1.
    expect(isDurable('nexus.memory.bank.v1')).toBe(true)
  })
})
