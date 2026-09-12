import type { MemoriesBank } from '../features/memories'
import { memoryBank, MEMORY_BANK_TTL_MS } from '../remote-native/memoryBank'
import type { QueryPage } from './application-query-protocol'

export type RemoteMemoryBank = { bank: MemoriesBank; sourceAgeMs: number; capturedAgeMs: number }
export function parseMemories(page: QueryPage): RemoteMemoryBank {
  const meta = page.meta as Record<string, unknown> | null
  const value = meta?.source as Record<string, unknown> | null
  if (page.collection !== 'memories' || page.offset !== 0 || page.total !== 0 || page.retained !== 0 || page.rows.length !== 0 || page.nextCursor !== null ||
    !meta || !Number.isSafeInteger(meta.capturedAgeMs) || Number(meta.capturedAgeMs) < 0 || Number(meta.capturedAgeMs) >= MEMORY_BANK_TTL_MS ||
    !value || Array.isArray(value) || Object.keys(value).length !== 2 || !Number.isSafeInteger(value.sourceAgeMs) ||
    Number(value.sourceAgeMs) < 0 || Number(value.sourceAgeMs) >= MEMORY_BANK_TTL_MS || !memoryBank(value.bank)) throw new Error('invalidMemories')
  return { bank: value.bank, sourceAgeMs: Number(value.sourceAgeMs), capturedAgeMs: Number(meta.capturedAgeMs) }
}
