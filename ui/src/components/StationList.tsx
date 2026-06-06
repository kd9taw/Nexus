import { useMemo, useState } from 'react'
import type { Station } from '../types'
import { StationCard } from './StationCard'

type Filter = 'all' | 'heard-now' | 'beaconing'

interface Props {
  stations: Station[]
  myGrid: string
  currentSlot: number
  activePeer: string | null
  unreadByPeer: Record<string, number>
  onSelect: (call: string) => void
  onCall: (call: string) => void
}

const FILTERS: { id: Filter; label: string }[] = [
  { id: 'all', label: 'All' },
  { id: 'heard-now', label: 'Heard now' },
  { id: 'beaconing', label: 'Beaconing' },
]

export function StationList({
  stations,
  myGrid,
  currentSlot,
  activePeer,
  unreadByPeer,
  onSelect,
  onCall,
}: Props) {
  const [filter, setFilter] = useState<Filter>('all')

  const filtered = useMemo(() => {
    let list = stations
    if (filter === 'heard-now') list = list.filter((s) => s.presence === 'active')
    else if (filter === 'beaconing') list = list.filter((s) => s.heardCount >= 3)
    // sort: presence (active first), then strongest SNR
    const order: Record<string, number> = { active: 0, idle: 1, stale: 2 }
    return [...list].sort(
      (a, b) => order[a.presence] - order[b.presence] || b.snr - a.snr,
    )
  }, [stations, filter])

  return (
    <aside className="station-list panel">
      <div className="panel-header">
        <h2>Stations</h2>
        <span className="count-badge">{stations.length}</span>
      </div>
      <div className="filter-row" role="tablist" aria-label="Station filter">
        {FILTERS.map((f) => (
          <button
            key={f.id}
            type="button"
            role="tab"
            aria-selected={filter === f.id}
            className={`filter-chip${filter === f.id ? ' active' : ''}`}
            onClick={() => setFilter(f.id)}
          >
            {f.label}
          </button>
        ))}
      </div>
      <div className="station-scroll">
        {filtered.length === 0 && <p className="empty">No stations match.</p>}
        {filtered.map((s) => (
          <StationCard
            key={s.call}
            station={s}
            myGrid={myGrid}
            currentSlot={currentSlot}
            selected={s.call === activePeer}
            unread={unreadByPeer[s.call] ?? 0}
            onSelect={onSelect}
            onCall={onCall}
          />
        ))}
      </div>
    </aside>
  )
}
