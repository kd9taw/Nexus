// Connect — the unified situational-awareness surface. The grayline map and the
// live propagation nowcast are TWO VIEWS OF ONE STATE: both read the same prop
// snapshot, operator grid, heard stations, need-state, and selection lifted in
// App. Selecting a station on the map highlights its great-circle path here; the
// sidebar's hero verdict + space-wx + band ladder answer "what's open, where to
// point, what do I need" at a glance. Map deep-dive + full Propagation panel
// remain available as their own sections within the Connect area.
import type { NeedTag, PropagationSnapshot, Station } from '../types'
import type { Theme } from '../useTheme'
import { MapView } from './MapView'
import { StateBlock } from './StateBlock'
import { SpaceWxGauges } from './prop/SpaceWxGauges'
import { BandAdvisor } from './prop/BandAdvisor'
import { OpeningStrip } from './prop/OpeningStrip'

interface Props {
  myGrid: string
  theme: Theme
  stations: Station[]
  prop: PropagationSnapshot | null
  selectedCall: string | null
  onSelectCall: (call: string | null) => void
  needByCall: Map<string, NeedTag>
}

function provLabel(source: PropagationSnapshot['source'], asOf: number): { label: string; cls: string } {
  if (source === 'live') return { label: 'LIVE', cls: 'live' }
  if (source === 'cached') {
    const m = Math.max(0, Math.round((Date.now() / 1000 - asOf) / 60))
    return { label: `CACHED ${m}m`, cls: 'cached' }
  }
  return { label: 'DEMO', cls: 'demo' }
}

export function ConnectView({
  myGrid,
  theme,
  stations,
  prop,
  selectedCall,
  onSelectCall,
  needByCall,
}: Props) {
  const prov = prop ? provLabel(prop.source, prop.asOf) : null
  return (
    <main className="layout single">
      <div className="connect">
        <div className="connect-map">
          <MapView
            myGrid={myGrid}
            theme={theme}
            stations={stations}
            prop={prop}
            selectedCall={selectedCall}
            onSelectCall={onSelectCall}
            needByCall={needByCall}
          />
        </div>
        <aside className="connect-side">
          {!prop ? (
            <StateBlock kind="loading" title="Reading the band…" detail="Fetching the propagation nowcast." />
          ) : (
            <>
              <div className="connect-hero-row">
                <div className="connect-hero">{prop.advisory.headline}</div>
                {prov && (
                  <span className={`prop-prov prov-${prov.cls}`} title="Data provenance">
                    {prov.label}
                  </span>
                )}
              </div>
              {prop.advisory.banners.map((b, i) => (
                <div key={i} className="prop-banner warn">
                  {b}
                </div>
              ))}
              <OpeningStrip openings={prop.openings} />
              <SpaceWxGauges wx={prop.spaceWx} />
              <BandAdvisor bands={prop.advisory.bands} />
            </>
          )}
        </aside>
      </div>
    </main>
  )
}
