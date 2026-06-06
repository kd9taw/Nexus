// Loud 6 m/VHF opening alerts — the under-served win, given the highest-salience
// treatment. Only rendered when openings exist.
import { Zap } from 'lucide-react'
import type { OpeningView } from '../../types'

export function OpeningStrip({ openings }: { openings: OpeningView[] }) {
  if (openings.length === 0) return null
  return (
    <div className="opening-strips">
      {openings.map((o, i) => (
        <div className="opening-strip" key={i}>
          <span className="opening-band">
            <Zap size={15} strokeWidth={2.25} aria-hidden="true" />
            {o.band} OPEN
          </span>
          <span className="opening-mode">{o.mode}</span>
          <span className="opening-detail">
            point {o.octant} · ~{Math.round(o.maxKm).toLocaleString()} km · {o.stations} stations · {o.confidence}
          </span>
          {o.note && <span className="opening-note">{o.note}</span>}
        </div>
      ))}
    </div>
  )
}
