import { useContext, useEffect, useRef, useState } from 'react'
import type { LoggedQso } from '../types'
import { useStationData } from '../stationAccess'
import { RemoteCollectionsContext } from './collections'
import type { QueryPage } from './application-query-protocol'

const EMPTY: LoggedQso[] = []
export function useRemoteLog(search: string, unconfirmed: boolean) {
  const generation = useRef(0)
  const source = useContext(RemoteCollectionsContext)
  const available = useStationData()
  const [pages, setPages] = useState<QueryPage[]>([])
  const [index, setIndex] = useState(0)
  const [phase, setPhase] = useState<'loading' | 'ready' | 'unavailable'>('loading')
  const [revision, setRevision] = useState(0)
  const refresh = () => setRevision(v => v + 1)
  useEffect(() => {
    const current = ++generation.current
    if (!source) return
    setPages([]); setIndex(0)
    if (!available) { setPhase('unavailable'); return }
    setPhase('loading')
    let live = true
    const timer = setTimeout(() => {
      void source.page({ collection: 'log', cursor: null, search: search.trim(), unconfirmed, after: null })
        .then(page => { if (live && current === generation.current) { setPages([page]); setPhase('ready') } })
        .catch(() => { if (live && current === generation.current) setPhase('unavailable') })
    }, 300)
    return () => { live = false; generation.current++; clearTimeout(timer) }
  }, [source, search, unconfirmed, revision, available])
  const page = pages[index]
  const next = async () => {
    if (!source || phase !== 'ready' || !page?.nextCursor || !available) return
    if (pages[index + 1]) { setIndex(index + 1); return }
    const current = generation.current
    setPhase('loading')
    try {
      const next = await source.page({ collection: 'log', cursor: page.nextCursor, search: search.trim(), unconfirmed, after: null })
      if (current !== generation.current) return
      if (next.snapshotId !== page.snapshotId || next.offset !== page.offset + page.rows.length || next.total !== page.total || next.retained !== page.retained) throw new Error('invalidApplicationPage')
      setPages(current => [...current, next])
      setIndex(index + 1)
      setPhase('ready')
    } catch { if (current === generation.current) { setPages([]); setIndex(0); setPhase('unavailable') } }
  }
  return source ? { rows: phase === 'ready' && page ? page.rows as unknown as LoggedQso[] : EMPTY,
    phase, total: page?.total ?? 0, retained: page?.retained ?? 0, offset: page?.offset ?? 0,
    hasPrevious: index > 0, hasNext: !!page?.nextCursor,
    previous: () => { if (phase === 'ready') setIndex(i => Math.max(0, i - 1)) }, next, refresh } : null
}
