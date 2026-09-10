import { createContext, useContext, useSyncExternalStore } from 'react'
import type { ApplicationTransport } from '../applicationTransport'
import type { DecodeRow, Tier } from '../types'
import { t } from '../i18n'
import type { Json } from './application-protocol'
import { NAVIGATION_COMMAND, navigationCollection, SSTV_IMAGE_COMMAND, APRS_COMMAND, JS8_CONTEXT_COMMAND, FIELD_DAY_COMMAND, OTA_COMMAND, MEMORIES_COMMAND, DXPEDITIONS_COMMAND, INSIGHTS_COMMAND, QUERY_COMMAND, RECALL_COMMAND, insightCollection } from './application-query-protocol'
import type { Collection, QueryArgs, QueryPage } from './application-query-protocol'
import type { ApplicationClient } from './application-client'

type State = { phase: 'loading' | 'ready' | 'unavailable'; total: number; retained: number; at: number }
const INITIAL: State = { phase: 'loading', total: 0, retained: 0, at: 0 }
type Whole = { rows: Json[]; meta: QueryPage['meta'] }
const READS: Record<string, Collection> = { get_need_alerts: 'needs', get_all_spots: 'spots', get_feed_health: 'health', dxcc_entity_locations: 'entities' }
export type HistoryRow = { sequence: number; firstSequence: number; slot: number; at: number; row: DecodeRow }
export type RemoteHistory = { rows: HistoryRow[]; generation: string; band: string; tier: Tier; dropped: number }
export const RemoteHistoryContext = createContext<RemoteHistory | null>(null)
export const RemoteCollectionsContext = createContext<RemoteCollections | null>(null)

export class RemoteCollections implements ApplicationTransport {
  readonly kind = 'remote' as const
  private states = new Map<Collection, State>()
  private cache = new Map<Collection, Whole>()
  private jobs = new Map<Collection, Promise<Whole>>()
  private listeners = new Set<() => void>()
  private live = true
  private generation = 0
  constructor(readonly client: ApplicationClient) {}
  subscribe = (listener: () => void): (() => void) => { this.listeners.add(listener); return () => { this.listeners.delete(listener) } }
  state = (name: Collection): State => this.states.get(name) ?? INITIAL
  private update(name: Collection, state: State): void { this.states.set(name, state); for (const f of this.listeners) f() }
  activate(): void { this.live = true }
  invalidate(): void { this.generation++; this.cache.clear(); this.jobs.clear(); for (const name of this.states.keys()) this.update(name, { ...INITIAL, phase: 'unavailable' }) }
  dispose(): void { this.generation++; this.live = false; this.cache.clear(); this.jobs.clear(); this.states.clear(); this.listeners.clear() }
  async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
    // This is the existing QRZ navigation gesture, handled in the user's browser.
    // It never reaches the station, its credential store, or a generic URL opener.
    if (command === 'open_qrz_page' && this.client.getPhase() === 'ready' && args && Object.keys(args).length === 1 && typeof args.call === 'string' && args.call.length <= 64) {
      const base = args.call.split('/').reduce((a, b) => b.length >= a.length ? b : a, '').replace(/[^a-z0-9]/gi, '').toUpperCase()
      if (!base) throw new Error('applicationUnsupported')
      window.open(`https://www.qrz.com/db/${base}`, '_blank', 'noopener,noreferrer')
      return undefined as T
    }
    const name = READS[command]
    if (!name || !this.client.supports(QUERY_COMMAND)) return this.client.invoke<T>(command, args)
    if (args && Object.keys(args).length) throw new Error('applicationUnsupported')
    const result = await this.read(name)
    return structuredClone(name === 'health' ? (result.meta as { source: Json }).source : result.rows) as T
  }
  async page(args: QueryArgs): Promise<QueryPage> {
    const generation = this.generation
    for (let attempt = 0; attempt < 3; attempt++) {
      try {
        if (!this.live || generation !== this.generation) throw new Error('applicationUnavailable')
        const page = await this.client.invoke<QueryPage>(navigationCollection(args.collection) ? NAVIGATION_COMMAND : args.collection === 'sstvImage' ? SSTV_IMAGE_COMMAND : args.collection === 'aprs' ? APRS_COMMAND : args.collection === 'js8Context' ? JS8_CONTEXT_COMMAND : args.collection === 'fieldDay' ? FIELD_DAY_COMMAND : args.collection === 'ota' ? OTA_COMMAND : args.collection === 'memories' ? MEMORIES_COMMAND : args.collection === 'dxpeditions' ? DXPEDITIONS_COMMAND : args.collection === 'recall' ? RECALL_COMMAND : insightCollection(args.collection) ? INSIGHTS_COMMAND : QUERY_COMMAND, args)
        if (!this.live || generation !== this.generation) throw new Error('applicationUnavailable')
        return page
      }
      catch (error) {
        if (attempt === 2 || !(error instanceof Error) || error.message !== 'applicationBusy') throw error
        await new Promise(resolve => setTimeout(resolve, 250))
      }
    }
    throw new Error('applicationUnavailable')
  }
  read(name: Collection, after: number | null = null): Promise<Whole> {
    const ttl = name === 'entities' ? 3600000 : name === 'needs' || name === 'health' ? 15000 : 5000
    const cached = this.cache.get(name)
    if (name !== 'decodes' && cached && performance.now() - this.state(name).at < ttl) return Promise.resolve(cached)
    const job = this.jobs.get(name)
    if (job) return job
    const pending = this.load(name, after)
    this.jobs.set(name, pending)
    void pending.finally(() => { if (this.jobs.get(name) === pending) this.jobs.delete(name) }).catch(() => {})
    return pending
  }
  private async load(name: Collection, after: number | null): Promise<Whole> {
    const generation = this.generation
    try {
      for (let attempt = 0; attempt < 2; attempt++) {
        try {
          let cursor: string | null = null, first: QueryPage | null = null
          const rows: Json[] = []
          do {
            const page = await this.page({ collection: name, cursor, search: '', unconfirmed: false, after })
            if (!this.live || generation !== this.generation) throw new Error('applicationUnavailable')
            if (page.offset !== rows.length || (first && (page.snapshotId !== first.snapshotId || page.total !== first.total || page.retained !== first.retained))) throw new Error('invalidApplicationPage')
            first ??= page; rows.push(...page.rows); cursor = page.nextCursor
          } while (cursor)
          const result = { rows, meta: first!.meta }
          if (name !== 'decodes') this.cache.set(name, result)
          this.update(name, { phase: 'ready', total: first!.total, retained: first!.retained, at: performance.now() })
          return result
        } catch (error) { if (attempt === 1 || !(error instanceof Error) || error.message !== 'queryExpired') throw error }
      }
      throw new Error('applicationUnavailable')
    } catch (error) {
      if (this.live && generation === this.generation) { this.cache.delete(name); this.update(name, { ...INITIAL, phase: 'unavailable' }) }
      throw error
    }
  }
}
export function useRemoteCollection(name: Collection): State | null {
  const source = useContext(RemoteCollectionsContext)
  const state = useSyncExternalStore(source?.subscribe ?? (() => () => {}), () => source?.state(name) ?? INITIAL, () => INITIAL)
  return source ? state : null
}
export function CollectionStatus({ name }: { name: Collection }) {
  const state = useRemoteCollection(name)
  if (!state) return null
  return <span role="status" className="dim">{state.phase === 'loading' ? t('remote.collectionLoading') : state.phase === 'unavailable'
    ? t('remote.collectionUnavailable') : state.retained < state.total ? t('remote.collectionCapped', { count: state.retained, total: state.total }) : t('remote.collectionObserver')}</span>
}
