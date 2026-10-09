// The Connect pane registry. Each PaneDef has a Basic projection (one sentence, from
// paneFormat) and an Expert render (the existing ConnectView panel JSX, copied verbatim
// with locals → c.*, so all .cs-*/.cp-*/.getout-* CSS keeps working untouched). When a
// pane has nothing live to show it either returns null → PaneFrame falls back to basic()
// (the honest loading/OFFLINE state — prop-derived panes return null on source==='offline'
// so modeled defaults never render as if live), or renders its own inline empty state
// (getout). Never render a modelled snapshot as live data.
// ⚠️ An `expert` that returns a COMPONENT hands the frame an element, never a null, so the
// fallback cannot reach it: such a component draws its box's one line itself (`.pane-basic`)
// when it has nothing — Openings Log, Chase, Chase Feed, Satellite Passes, Rotor, Amplifier and
// Contests do. PaneBody.test.tsx renders every box with nothing to show and fails on an empty body.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Each pane's NAME
// resolves through a getter, exactly as `features/needVisuals.ts` does: this registry is a
// module constant that PaneFrame reads during render, so resolving at import time would
// freeze whichever locale loaded first. The record shape is unchanged, so no consumer moved.
// What stays in the code: band, mode and octant names, MUF and spot frequencies, bearings,
// distances, SNR, CQ zones, grids, the `P.533` recommendation number, and every word the
// BACKEND sends (the advisory headline and its banners, the workability, the window text).
// No transmit control renders on any of these panes — ▶ Work moves the rig and opens a
// cockpit; it keys nothing. The Spots, POTA/SOTA and Needed boxes are the boards themselves, and
// their Work and HUNT are the boards' own (the window lends its wiring through the context).
import type { ReactNode } from 'react'
import { t } from '../../i18n'
import { T } from '../../i18n/T'
import { type PaneId, PANE_IDS } from '../../features/connectConfig'
import type { PaneContext } from './paneContext'
import type { BandOutlook } from '../../types'
import { bandTiming } from '../../propViz'
import { azimuthLabel, azimuthTitle } from '../../grid'
import { SpaceWxGauges } from '../prop/SpaceWxGauges'
import { BandAdvisor } from '../prop/BandAdvisor'
import { BandTiles } from '../prop/BandTiles'
import { OpeningStrip } from '../prop/OpeningStrip'
import { LikelihoodHeatmap } from '../prop/LikelihoodHeatmap'
import { BestBandTable } from '../prop/BestBandTable'
import { ActivityMatrix } from '../prop/ActivityMatrix'
import { BeaconMonitor } from '../prop/BeaconMonitor'
import { InsightFeed } from '../prop/InsightFeed'
import { ChasePane } from '../prop/ChasePane'
import { ChaseFeedPane } from '../prop/ChaseFeedPane'
import { SatPassesPane } from '../prop/SatPassesPane'
import { OpeningsLogPane } from '../prop/OpeningsLogPane'
import { KpOutlookPane } from '../prop/KpOutlookPane'
import { SolarTrends } from '../prop/SolarTrends'
import { ClockPane } from '../prop/ClockPane'
import { RotorPane } from '../prop/RotorPane'
import { AmpPane } from '../prop/AmpPane'
import { MiniSpectrum } from '../MiniSpectrum'
import { ContestCalendarPane } from '../ContestCalendarPane'
import { getContests } from '../../api'
import { GetoutCompass } from '../prop/GetoutCompass'
import { getoutSummary } from '../../features/getout'
import { fmtDistanceKm } from '../../units'
import { GreylineWindow } from '../prop/GreylineWindow'
import { ScalesAnnunciator } from '../prop/ScalesAnnunciator'
import { MeasuredMuf } from '../prop/MeasuredMuf'
import { PotaSotaView } from '../PotaSotaView'
import { ageLabel } from '../SpotsPanel'
import { SpotsBox } from './SpotsBox'
import { NeededBox } from './NeededBox'
import {
  NEED_CHIP,
  dxpedWorkMode,
  advisoryLine,
  bandAdvisorLine,
  selectionLine,
  selectionAzimuth,
  selectionEntity,
  outlookLine,
  openingsLine,
  spaceWxLine,
  getoutLine,
  bestbandLine,
  activityLine,
  beaconsLine,
  insightsLine,
  chaseLine,
  chaseFeedLine,
  greylineLine,
  bandHoursLine,
  esNowcastLine,
  measuredMufLine,
} from './paneFormat'

/** What a box is FOR — the groups its picker and ⋯ ▸ Add a tab list it under, in this order (the
 *  operator's pick, 2026-09-30: "Name by purpose"). They were the build tiers that added the boxes
 *  ("Panels", "B2", "B3"), which put Chase, the flagship, under "B2". */
export const PANE_CATEGORIES = ['bands', 'spaceWx', 'activity', 'station'] as const
export type PaneCategory = (typeof PANE_CATEGORIES)[number]

/** Each group's heading. Literal keys, resolved lazily at render (the registry's getter treatment). */
export const PANE_CATEGORY_LABEL: Record<PaneCategory, () => string> = {
  bands: () => t('connect.slot.group.bands'),
  spaceWx: () => t('connect.slot.group.spaceWx'),
  activity: () => t('connect.slot.group.activity'),
  station: () => t('connect.slot.group.station'),
}

/** The ITU recommendation's number — a document name, the same in every language. */
const ENGINE_P533 = 'P.533'

/** The POTA/SOTA box's name: the two programmes' own names, as the POTA/SOTA view's title prints
 *  them — tokens, never a catalog entry. */
const OTA_BOX_TITLE = 'POTA / SOTA'

/** Unit symbols printed beside a reading. A unit is a token — MHz is MHz and dB is dB in every
 * language — and the guard is told so by these constants. A distance carries its unit from
 * `fmtDistanceKm`, in the units the operator chose. */
const MHZ_UNIT = 'MHz'
const DB_UNIT = 'dB'

export interface PaneDef {
  id: PaneId
  title: string
  category: PaneCategory
  /** ONE plain sentence projected from ctx — also the empty/loading/offline hint. */
  basic: (c: PaneContext) => string
  /** Full panel: the unchanged existing JSX, reading c.*. Returns null when no data
   *  (PaneFrame then falls back to basic(), so loading/offline needs no special case). */
  expert: (c: PaneContext) => ReactNode
}

// ---- Expert renders: verbatim ConnectView panel JSX, locals → c.* ----

/** Per-mode "workable now" chips (P.533 only — the heuristic sends none).
 * Same Fair boundary (0.3) as everywhere; ≥0.5 reads as solidly workable. */
function ModeNowChips({ b }: { b: BandOutlook }) {
  if (!b.modeNow?.length) return null
  return (
    <span className="cp-modes">
      {b.modeNow.map((m) => {
        const cls = m.score >= 0.5 ? 'good' : m.score >= 0.3 ? 'fair' : 'closed'
        return (
          <span
            key={m.mode}
            className={`cp-mode ${cls}`}
            title={t('connect.path.modeChip.title', {
              mode: m.mode,
              pct: Math.round(m.score * 100),
            })}
          >
            {m.mode}
          </span>
        )
      })}
    </span>
  )
}

function renderSelection(c: PaneContext): ReactNode {
  const call = c.selectedCall
  if (!call) return null
  return (
    <section className="connect-sel panel">
      <div className="cs-head">
        <b className="cs-call">{call}</b>
        {(() => {
          const tag = c.needByCall.get(call.toUpperCase())
          const chip = tag ? NEED_CHIP[tag] : null
          return chip ? <span className={`need-chip need-${chip.cls}`}>{chip.label}</span> : null
        })()}
        <button
          type="button"
          className="cs-close"
          onClick={() => c.onSelectCall(null)}
          title={t('connect.selection.clear.title')}
        >
          ✕
        </button>
      </div>
      <div className="cs-who">
        {c.selSpot?.entity ?? c.selDxped?.entity ?? c.selStation?.country ?? '—'}
        {(() => {
          const az = selectionAzimuth(c)
          return az ? (
            <span className="cs-az" title={azimuthTitle(az, selectionEntity(c))}>
              {' '}
              {azimuthLabel(az)}
            </span>
          ) : null
        })()}
        {c.selSpot?.cqZone != null && ` · CQ ${c.selSpot.cqZone}`}
        {c.selStation?.grid && ` · ${c.selStation.grid}`}
      </div>
      {c.selSpot && (
        <div className="cs-spot">
          {c.selSpot.band}
          {c.selSpot.mode ? ` ${c.selSpot.mode}` : ''}
          {c.selSpot.freqMhz
            ? ` · ${c.selSpot.freqMhz.toFixed(4).replace(/\.?0+$/, '')} ${MHZ_UNIT}`
            : ''}
          {' · '}
          {c.selSpot.ageSecs < 60
            ? t('connect.selection.age.secs', { secs: c.selSpot.ageSecs })
            : t('connect.selection.age.mins', { mins: Math.round(c.selSpot.ageSecs / 60) })}
          {c.selSpot.heardMe && ` · ${t('connect.selection.heardYou')}`}
          {c.selSpot.approx && ` · ${t('connect.selection.approx')}`}
        </div>
      )}
      {c.selStation && (
        <div className="cs-spot">
          {t('connect.selection.decoded')} · {c.selStation.snr} {DB_UNIT}
          {c.selStation.worked ? ` · ${t('connect.selection.workedBefore')}` : ''}
        </div>
      )}
      {c.selDxped && c.selDxpedWindow?.best && (
        <div className="cs-spot">
          {t('connect.selection.bestShot', { window: c.selDxpedWindow.best })}
          <span className="cp-engine">
            {c.selDxpedWindow.engine === 'p533' ? ENGINE_P533 : t('connect.engine.modelled')}
          </span>
        </div>
      )}
      {c.onWorkSpot && (c.selSpot || c.selDxped) && (
        <button
          type="button"
          className="cs-work"
          onClick={() =>
            c.selSpot
              ? c.onWorkSpot!({
                  call: c.selSpot.call,
                  band: c.selSpot.band,
                  mode: c.selSpot.mode ?? null,
                  freqMhz: c.selSpot.freqMhz ?? null,
                })
              : c.selDxped &&
                c.onWorkSpot!({
                  call: c.selDxped.call,
                  band: c.selDxped.band,
                  mode: dxpedWorkMode(c.selDxped.modes),
                  freqMhz: null,
                })
          }
          title={t('connect.selection.work.title')}
        >
          {t('connect.selection.work.label', {
            band: (c.selSpot ? c.selSpot.band : c.selDxped?.band) ?? '',
          })}
          {c.selSpot?.freqMhz ? ` @ ${c.selSpot.freqMhz.toFixed(4).replace(/\.?0+$/, '')}` : ''}
        </button>
      )}
    </section>
  )
}

function renderPath(c: PaneContext): ReactNode {
  const p = c.pathPred
  if (!p || !c.selectedCall) return null
  return (
    <section className="connect-path panel">
      <h3>
        {t('connect.path.heading', { call: c.selectedCall })}
        {p.engine && (
          <span className="cp-engine">
            {p.engine === 'heuristic'
              ? t('connect.engine.modelled')
              : p.engine === 'p533'
                ? ENGINE_P533
                : p.engine}
          </span>
        )}
      </h3>
      {p.mufNow > 0 && (
        <p className="cp-muf" title={t('connect.path.muf.title')}>
          <T
            k="connect.path.muf"
            vals={{ muf: p.mufNow.toFixed(1) }}
            tags={{ b: <strong /> }}
          />
        </p>
      )}
      {c.pathOpen.length === 0 ? (
        <p className="cp-none">{t('connect.path.none')}</p>
      ) : (
        <>
          <ul className="connect-path-list">
            {c.pathOpen.slice(0, 6).map((b) => (
              <li key={b.band}>
                <span className="cp-band">{b.band}</span>
                <span className={`cp-work w-${b.workability.toLowerCase()}`}>{b.workability}</span>
                <span className="cp-win">
                  {b.grayline && (
                    <span className="cp-grayline" title={t('connect.path.greyline.title')}>
                      ◐{' '}
                    </span>
                  )}
                  {b.window}
                </span>
                <span className="cp-eta">{bandTiming(b.hourly, Date.now())}</span>
                <ModeNowChips b={b} />
              </li>
            ))}
          </ul>
          <LikelihoodHeatmap outlook={c.pathOpen.slice(0, 6)} />
        </>
      )}
    </section>
  )
}

function renderOutlook(c: PaneContext): ReactNode {
  const o = c.bandOutlook
  if (!o) return null
  return (
    <section className="connect-path panel">
      <h3>
        {t('connect.outlook.heading')}
        <span className="cp-engine">{t('connect.engine.outlook')}</span>
      </h3>
      {o.mufNow > 0 && (
        <p className="cp-muf" title={t('connect.outlook.muf.title')}>
          <T
            k="connect.path.muf"
            vals={{ muf: o.mufNow.toFixed(1) }}
            tags={{ b: <strong /> }}
          />
        </p>
      )}
      {c.outlookOpen.length === 0 ? (
        <p className="cp-none">{t('connect.outlook.none')}</p>
      ) : (
        <>
          <ul className="connect-path-list">
            {c.outlookOpen.slice(0, 8).map((b) => (
              <li key={b.band}>
                <span className="cp-band">{b.band}</span>
                <span className={`cp-work w-${b.workability.toLowerCase()}`}>{b.workability}</span>
                <span className="cp-win">
                  {b.grayline && (
                    <span className="cp-grayline" title={t('connect.path.greyline.title')}>
                      ◐{' '}
                    </span>
                  )}
                  {b.window}
                </span>
                <span className="cp-eta">{bandTiming(b.hourly, Date.now())}</span>
                <ModeNowChips b={b} />
              </li>
            ))}
          </ul>
          <LikelihoodHeatmap outlook={c.outlookOpen.slice(0, 8)} />
        </>
      )}
    </section>
  )
}

function renderGetout(c: PaneContext): ReactNode {
  const g = c.getout
  return (
    <section className="connect-getout panel">
      <h3>{t('connect.getout.heading')}</h3>
      {!g || g.count === 0 ? (
        <p className="cp-none">{t('connect.getout.none')}</p>
      ) : (
        <>
          <p className="getout-summary">
            <T
              k="connect.getout.summary"
              vals={{ count: g.count, dist: fmtDistanceKm(g.maxKm, c.units) }}
              tags={{ b: <strong /> }}
            />
          </p>
          <div className="getout-rose-wrap">
            <GetoutCompass reports={g.reports} maxKm={g.maxKm} units={c.units} />
            <p className="getout-dir">{getoutSummary(g.reports, c.units)}</p>
          </div>
          {/* THE FULL LIST: every receiver the station reports (`propagation::getting_out` — the
              latest report per receiver over its window, most distant first), not the first six.
              It scrolls inside the box's body; each row gains how long ago it heard you. */}
          <ul className="getout-list">
            {g.reports.map((r) => (
              <li
                key={r.call}
                className="go-clickable"
                onClick={() => c.onSelectCall(r.call)}
                title={t('connect.getout.select.title', { call: r.call })}
              >
                <span className="go-call">{r.call}</span>
                <span className="go-where">
                  {r.octant} {fmtDistanceKm(r.km, c.units)}
                </span>
                <span className="go-band">{r.band}</span>
                <span className="go-snr">{r.snr != null ? `${r.snr} ${DB_UNIT}` : ''}</span>
                <span className="go-age">{ageLabel(r.ageSecs)}</span>
              </li>
            ))}
          </ul>
        </>
      )}
    </section>
  )
}

export const PANES: PaneDef[] = [
  {
    id: 'advisory',
    get title() {
      return t('connect.pane.advisory.title')
    },
    category: 'bands',
    basic: advisoryLine,
    // Offline → null → Basic's honest "No live propagation data" (never the modelled
    // headline/banners as if live). Ternary, not &&, so offline yields null not false.
    expert: (c) =>
      c.prop && c.prop.source !== 'offline' ? (
        <>
          <div className="connect-hero-row">
            <div className="connect-hero">{c.prop.advisory.headline}</div>
            {c.prov && (
              <span className={`prop-prov prov-${c.prov.cls}`} title={t('connect.prov.title')}>
                {c.prov.label}
              </span>
            )}
          </div>
          {c.prop.advisory.banners.map((b, i) => (
            <div key={i} className="prop-banner warn">
              {b}
            </div>
          ))}
        </>
      ) : null,
  },
  {
    id: 'bandAdvisor',
    get title() {
      return t('connect.pane.bandAdvisor.title')
    },
    category: 'bands',
    basic: bandAdvisorLine,
    expert: (c) =>
      c.prop && c.prop.source !== 'offline' ? (
        <BandAdvisor
          bands={c.prop.advisory.bands}
          worldwideBands={c.prop.worldwide?.bands ?? null}
          onBandClick={c.toggleFocusBand}
          activeBand={c.focusBand}
        />
      ) : null,
  },
  {
    // BANDS FOR YOU — the band advice as tiles, one per band, in the Band Advisor's default slot.
    // With a snapshot it always draws: offline or stale data gives hollow neutral tiles, never green
    // (features/bandTiles). Before the first snapshot it returns null, and Basic says so.
    id: 'bandTiles',
    get title() {
      return t('connect.pane.bandTiles.title')
    },
    category: 'bands',
    basic: bandAdvisorLine,
    expert: (c) =>
      c.prop ? (
        <BandTiles
          prop={c.prop}
          outlook={c.bandOutlook?.bands}
          rigBand={c.rigBand}
          focusBand={c.focusBand}
          onBandClick={c.toggleFocusBand}
        />
      ) : null,
  },
  {
    id: 'selection',
    get title() {
      return t('connect.pane.selection.title')
    },
    category: 'activity',
    basic: selectionLine,
    expert: (c) => renderSelection(c),
  },
  {
    id: 'outlook',
    get title() {
      return t('connect.pane.outlook.title')
    },
    category: 'bands',
    basic: outlookLine,
    // Selection-aware: path-to-the-selected-call, else band-outlook-to-DX (same JSX shape).
    expert: (c) => (c.selectedCall ? renderPath(c) : renderOutlook(c)),
  },
  {
    id: 'openings',
    get title() {
      return t('connect.pane.openings.title')
    },
    category: 'bands',
    basic: openingsLine,
    expert: (c) =>
      c.prop && c.prop.source !== 'offline' ? (
        <OpeningStrip openings={c.prop.openings} onBandClick={c.toggleFocusBand} />
      ) : null,
  },
  {
    id: 'openingsLog',
    get title() {
      return t('connect.pane.openingsLog.title')
    },
    category: 'bands',
    // Self-fetching pane (get_openings_log) — the Basic line stays a static
    // honest hint because the history lives inside the component, which draws it.
    basic: () => t('connect.pane.openingsLog.basic'),
    expert: () => <OpeningsLogPane />,
  },
  {
    id: 'kpOutlook',
    get title() {
      return t('connect.pane.kpOutlook.title')
    },
    category: 'spaceWx',
    // Self-fetching (get_kp_forecast, cached 15 min server-side), so the Basic line
    // is a static honest hint rather than a value this context does not carry.
    basic: () => t('connect.pane.kpOutlook.basic'),
    expert: () => <KpOutlookPane />,
  },
  {
    id: 'spacewx',
    get title() {
      return t('connect.pane.spacewx.title')
    },
    category: 'spaceWx',
    basic: spaceWxLine,
    expert: (c) =>
      c.prop && c.prop.source !== 'offline' ? (
        <>
          <SpaceWxGauges wx={c.prop.spaceWx} gloss={false} />
          {/* Self-fetching (get_solar_indices, cached an hour server-side): SSN and the 30-day
              SFI/SSN lines, stale-honest by the file's own dates. */}
          <SolarTrends />
          <ScalesAnnunciator scales={c.scales} alerts={c.alerts} />
        </>
      ) : null,
  },
  {
    id: 'getout',
    get title() {
      return t('connect.pane.getout.title')
    },
    category: 'activity',
    basic: getoutLine,
    expert: (c) => renderGetout(c),
  },
  // ---- B2 Tier-1 panes (pickable; DEFAULT_SLOTS keeps the approved core layout) ----
  {
    id: 'bestband',
    get title() {
      return t('connect.pane.bestband.title')
    },
    category: 'bands',
    basic: bestbandLine,
    expert: (c) => {
      const rows = c.prop && c.prop.source !== 'offline' ? c.prop.bestToRegion : undefined
      return rows?.length ? (
        <BestBandTable rows={rows} onBandClick={c.toggleFocusBand} activeBand={c.focusBand} />
      ) : null
    },
  },
  {
    id: 'activity',
    get title() {
      return t('connect.pane.activity.title')
    },
    category: 'activity',
    basic: activityLine,
    expert: (c) => {
      const cells = c.prop && c.prop.source !== 'offline' ? c.prop.regionBand : undefined
      return cells?.length ? (
        <ActivityMatrix cells={cells} onBandClick={c.toggleFocusBand} activeBand={c.focusBand} />
      ) : null
    },
  },
  {
    id: 'beacons',
    get title() {
      return t('connect.pane.beacons.title')
    },
    category: 'bands',
    basic: beaconsLine,
    // Clock-derived — never gates on offline; only the heard badges need spots.
    expert: (c) => <BeaconMonitor spots={c.prop?.spots ?? null} />,
  },
  {
    id: 'insights',
    get title() {
      return t('connect.pane.insights.title')
    },
    category: 'spaceWx',
    basic: insightsLine,
    expert: (c) => {
      const ins = c.prop && c.prop.source !== 'offline' ? c.prop.insights : undefined
      return ins?.length ? (
        <InsightFeed insights={ins} onBandClick={c.toggleFocusBand} />
      ) : null
    },
  },
  {
    id: 'chase',
    get title() {
      return t('connect.pane.chase.title')
    },
    category: 'activity',
    basic: chaseLine,
    // "Work THIS now": needed stations fused with band openness + window. When nothing's
    // needed-and-heard the pane draws this Basic line itself (the same sentence).
    expert: (c) => <ChasePane ctx={c} />,
  },
  // ---- B3 Tier-2 no-network panes (pickable; reuse existing snapshot data) ----
  {
    id: 'greyline',
    get title() {
      return t('connect.pane.greyline.title')
    },
    category: 'spaceWx',
    basic: greylineLine,
    // Clock-derived; GreylineWindow handles the no-grid case itself (never null).
    expert: (c) => <GreylineWindow ctx={c} />,
  },
  {
    id: 'bandHours',
    get title() {
      return t('connect.pane.bandHours.title')
    },
    category: 'bands',
    basic: bandHoursLine,
    expert: (c) =>
      c.bandOutlook?.bands.length ? <LikelihoodHeatmap outlook={c.bandOutlook.bands} /> : null,
  },
  {
    id: 'esNowcast',
    get title() {
      return t('connect.pane.esNowcast.title')
    },
    category: 'bands',
    basic: esNowcastLine,
    expert: (c) => {
      // VHF openings → the cards; otherwise null so PaneFrame falls back to the (identical)
      // Basic season line — no duplicated empty state.
      const vhf = (c.prop?.openings ?? []).filter((o) => ['6m', '4m', '2m'].includes(o.band))
      return vhf.length ? <OpeningStrip openings={vhf} onBandClick={c.toggleFocusBand} /> : null
    },
  },
  {
    id: 'measuredMuf',
    get title() {
      return t('connect.pane.measuredMuf.title')
    },
    category: 'spaceWx',
    basic: measuredMufLine,
    expert: (c) => {
      const m = c.muf ?? []
      return m.length ? <MeasuredMuf stations={m} /> : null
    },
  },
  {
    id: 'chaseFeed',
    get title() {
      return t('connect.pane.chaseFeed.title')
    },
    category: 'activity',
    basic: chaseFeedLine,
    // The ranked "chase tonight" board: heard needs + on-air expeditions fused and
    // scored (need × openness × rarity × time-remaining). Basic = top-3 plain rows;
    // Expert = the full table. Nothing chase-worthy → the pane draws this basic() line.
    expert: (c) => <ChaseFeedPane ctx={c} />,
  },
  {
    id: 'satPasses',
    get title() {
      return t('connect.pane.satPasses.title')
    },
    category: 'activity',
    // Self-fetching pane (get_satellites) — the Basic line stays a static honest
    // hint because the data lives inside the component, not PaneContext; the component draws it.
    basic: () => t('connect.pane.satPasses.basic'),
    expert: () => <SatPassesPane />,
  },
  {
    id: 'rotor',
    get title() {
      return t('connect.pane.rotor.title')
    },
    category: 'station',
    basic: () => t('connect.pane.rotor.basic'),
    // Self-contained control surface — polls read_rotator while mounted and draws only this
    // Basic hint itself when NO rotator is configured. A configured rotator that cannot
    // report its position keeps the pane and its STOP button; the hint used to name the
    // ADVANCED external-rotctld field, which is not where a rotator is set up. Beside a cockpit it
    // also shows the call in that cockpit's log entry, its bearing and a Point (`entryCall`).
    expert: (c) => <RotorPane entryCall={c.entryCall} />,
  },
  {
    id: 'amp',
    get title() {
      return t('connect.pane.amp.title')
    },
    category: 'station',
    // Read-only station-device readout, the rotor's site-for-site shape. The Basic hint is
    // STATIC and names where the amplifier is configured, because the pane draws it (and no
    // readout) only when none is — a configured amplifier that has gone quiet keeps the pane, with '—'.
    //
    // ⛔ It renders no control of any kind and stops nothing: an amplifier in standby does not
    // stop an over (the exciter keeps keying and the drive passes straight through). This pane
    // must never appear in any cockpit vocabulary or any sweep's `stopControls`.
    basic: () => t('connect.pane.amp.basic'),
    expert: (c) => <AmpPane amp={c.amp} />,
  },
  {
    id: 'scope',
    get title() {
      return t('connect.pane.scope.title')
    },
    category: 'station',
    basic: () => t('connect.pane.scope.basic'),
    expert: () => (
      <MiniSpectrum
        height={120}
        idleHint={t('connect.pane.scope.idle')}
      />
    ),
  },
  {
    id: 'contests',
    get title() {
      return t('connect.pane.contests.title')
    },
    category: 'activity',
    // Self-fetching (get_contests) — Basic stays a static hint since the data
    // lives in the component, not PaneContext, which draws it (same pattern as Satellite Passes).
    basic: () => t('connect.pane.contests.basic'),
    expert: () => <ContestCalendarPane load={getContests} />,
  },
  {
    id: 'clock',
    get title() {
      return t('connect.pane.clock.title')
    },
    category: 'station',
    // Clock-derived, like the beacons: never gates on a feed and never returns null, so the
    // Basic line is only a description.
    basic: () => t('connect.pane.clock.basic'),
    expert: (c) => <ClockPane myGrid={c.myGrid} />,
  },
  {
    // THE SPOTS BOX — the Spots board itself, with the window's own Spots wiring (SpotsBox). A
    // window with no Spots board to lend (the wall display) gets the Basic line, and no Work.
    id: 'spots',
    get title() {
      return t('connect.pane.spots.title')
    },
    category: 'activity',
    basic: () => t('connect.pane.spots.basic'),
    expert: (c) => (c.spotsFeed ? <SpotsBox feed={c.spotsFeed} /> : null),
  },
  {
    // THE POTA/SOTA BOX — the POTA/SOTA board itself as a box (`pane`): its list, filters, sort and
    // HUNT, with the window's own hunt wiring. Self-fetching, as the view is. No wiring ⇒ Basic.
    id: 'pota',
    title: OTA_BOX_TITLE,
    category: 'activity',
    basic: () => t('connect.pane.pota.basic'),
    expert: (c) =>
      c.otaBoard ? (
        <PotaSotaView snap={c.otaBoard.snap} onHunt={c.otaBoard.onHunt} onSnap={c.otaBoard.onSnap} pane />
      ) : null,
  },
  {
    // THE NEEDED BOX — the Needed board itself, with the window's own Needed wiring (NeededBox). In the
    // Spots and POTA/SOTA boxes' group. A window with none to lend gets the Basic line, and no Work.
    id: 'needed',
    get title() {
      return t('connect.pane.needed.title')
    },
    category: 'activity',
    basic: () => t('connect.pane.needed.basic'),
    expert: (c) => (c.neededBoard ? <NeededBox board={c.neededBoard} /> : null),
  },
]

export const PANE_BY_ID = new Map<PaneId, PaneDef>(PANES.map((p) => [p.id, p]))
export function paneById(id: PaneId): PaneDef | undefined {
  return PANE_BY_ID.get(id)
}

/** Structural invariants (exercised by panes.test.ts so a malformed registry fails CI). */
export function validatePaneRegistry(): string[] {
  const errs: string[] = []
  const seen = new Set<PaneId>()
  for (const p of PANES) {
    if (seen.has(p.id)) errs.push(`duplicate pane ${p.id}`)
    seen.add(p.id)
  }
  for (const id of PANE_IDS) if (!seen.has(id)) errs.push(`PANE_IDS has ${id} but PANES does not`)
  return errs
}
