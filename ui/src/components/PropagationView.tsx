import type { PropagationSnapshot } from '../types'
import { StateBlock } from './StateBlock'
import { SpaceWxGauges } from './prop/SpaceWxGauges'
import { BandAdvisor } from './prop/BandAdvisor'
import { OpeningStrip } from './prop/OpeningStrip'
import { WorkNowCard } from './prop/WorkNowCard'
import { DxpedCalendar } from './prop/DxpedCalendar'

interface Props {
  snap: PropagationSnapshot | null
}

function provenance(source: PropagationSnapshot['source'], asOf: number): { label: string; cls: string } {
  if (source === 'live') return { label: 'LIVE', cls: 'live' }
  if (source === 'cached') {
    const m = Math.max(0, Math.round((Date.now() / 1000 - asOf) / 60))
    return { label: `CACHED ${m}m`, cls: 'cached' }
  }
  return { label: 'DEMO DATA', cls: 'demo' }
}

/**
 * Propagation & Opening Intelligence — Mission-Control: a glanceable, plain-
 * language nowcast (hero verdict + space-weather gauges), loud 6m/VHF opening
 * alerts, a ranked band advisor, needed × workable-now DXpedition cards with
 * modelled likelihood, and the forward DXpedition calendar with the band×hour
 * likelihood heatmap. Provenance is always visible (never silently demo/stale).
 */
export function PropagationView({ snap }: Props) {
  if (!snap) {
    return (
      <div className="prop">
        <StateBlock kind="loading" title="Reading the band…" detail="Fetching the propagation nowcast." />
      </div>
    )
  }
  const { advisory, openings, dxpeditions, spaceWx, source, asOf } = snap
  const prov = provenance(source, asOf)

  return (
    <div className="prop">
      <div className="prop-hero-row">
        <div className="prop-hero">{advisory.headline}</div>
        <span className={`prop-prov prov-${prov.cls}`} title="Data provenance">
          {prov.label}
        </span>
      </div>

      {advisory.banners.map((b, i) => (
        <div key={i} className="prop-banner warn">
          {b}
        </div>
      ))}

      <OpeningStrip openings={openings} />
      <SpaceWxGauges wx={spaceWx} />

      <div className="prop-grid">
        <BandAdvisor bands={advisory.bands} />

        <aside className="prop-side">
          <section className="prop-dxped panel">
            <h2>DXpeditions — work now</h2>
            {dxpeditions.workableNow.length === 0 ? (
              <StateBlock
                kind="empty"
                title="Nothing workable right now"
                detail="No needed DXpedition is open on a workable band — check the calendar below."
              />
            ) : (
              dxpeditions.workableNow.map((c, i) => <WorkNowCard key={`${c.call}-${c.band}-${i}`} card={c} />)
            )}
          </section>
        </aside>
      </div>

      <DxpedCalendar entries={dxpeditions.upcoming} />
    </div>
  )
}
