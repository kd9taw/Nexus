import { describe, expect, it } from 'vitest'
import { importPack, STARTER_PACKS } from './packs'
import { emptyBank, coerceMemory } from './memories'

describe('starter packs', () => {
  it('every packed memory is valid (coerces cleanly)', () => {
    for (const pack of STARTER_PACKS) {
      for (const pm of pack.memories) {
        const m = coerceMemory({ ...pm, id: 'x' })
        expect(m, `${pack.name} / ${pm.name}`).not.toBeNull()
        expect(m?.rxMhz).toBeGreaterThan(0)
      }
    }
  })

  it('installs a pack into a named group, tagged curated', () => {
    const pack = STARTER_PACKS[0]
    const { bank, added } = importPack(emptyBank(), pack)
    expect(added).toBe(pack.memories.length)
    expect(bank.groups.some((g) => g.name === pack.name)).toBe(true)
    const gid = bank.groups.find((g) => g.name === pack.name)?.id
    expect(bank.memories.every((m) => m.source === 'curated')).toBe(true)
    expect(bank.memories.every((m) => gid && m.groups.includes(gid))).toBe(true)
  })

  it('is idempotent — re-installing adds nothing and does not duplicate the group', () => {
    const pack = STARTER_PACKS[0]
    const first = importPack(emptyBank(), pack)
    const second = importPack(first.bank, pack)
    expect(second.added).toBe(0)
    expect(second.bank.memories).toHaveLength(pack.memories.length)
    expect(second.bank.groups.filter((g) => g.name === pack.name)).toHaveLength(1)
  })

  it('adds a channel shared by two packs to BOTH packs’ groups', () => {
    const digital = STARTER_PACKS.find((p) => p.id === 'na-digital')!
    const pota = STARTER_PACKS.find((p) => p.id === 'na-pota')!
    let bank = importPack(emptyBank(), digital).bank
    bank = importPack(bank, pota).bank
    const potaGid = bank.groups.find((g) => g.name === pota.name)!.id
    // 14.074 FT8 lives in both packs (same memoryKey) — it must be tagged into the POTA group
    // even though it was first added by the Digital pack.
    const shared = bank.memories.find((m) => m.rxMhz === 14.074 && m.mode === 'FT8')
    expect(shared?.groups).toContain(potaGid)
  })

  it('carries a scheduled net through with its reminder schedule (default off)', () => {
    const nets = STARTER_PACKS.find((p) => p.id === 'na-nets')!
    const { bank } = importPack(emptyBank(), nets)
    const mmsn = bank.memories.find((m) => m.name.startsWith('Maritime'))
    expect(mmsn?.net?.days).toHaveLength(7)
    expect(mmsn?.net?.alertEnabled).toBe(false)
  })
})
