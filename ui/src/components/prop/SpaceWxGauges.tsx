// Space-weather strip: each index as value + severity bar + plain-language HF
// impact. The numbers stay visible (project rule: never hide the physics); the
// plain language is the Mission-Control glanceable layer. In Simple mode (`gloss`)
// each acronym carries a hover/tap plain-English definition so a newcomer is
// never staring at a cryptic "SFI 142 / Kp 4"; Expert mode assumes fluency.
import type { SolarWind, SpaceWxView } from '../../types'
import {
  sfiImpact,
  kpImpact,
  aImpact,
  xrayImpact,
  bzImpact,
  solarWindAgeSecs,
  SOLAR_WIND_STALE_SECS,
  type Impact,
} from '../../propViz'
import { Tooltip, TooltipProvider } from '../ui/Tooltip'
import { t, type MessageKey } from '../../i18n'

const SEV_VAR: Record<Impact['sev'], string> = {
  quiet: 'var(--band-open)',
  active: 'var(--band-marginal)',
  warn: 'var(--alert-warning)',
}

/** The index NAMES. Technical tokens — SFI is SFI on every ham's screen in every language,
 * exactly like a band or a mode name — so they live here, not in the catalog. What each one
 * MEANS is prose and is a catalog entry (`prop.spaceWx.gloss.*`), attached per gauge below.
 * Bz deliberately has no gloss: the shipped Simple-mode table never carried one. */
const INDEX = {
  sfi: 'SFI',
  kp: 'Kp',
  a: 'A',
  xray: 'X-ray',
  bz: 'Bz',
} as const

/** The value a gauge shows when there is no reading — a mark, not a word. */
const NO_VALUE = '—'

/** Plain-English glosses for the space-weather acronyms (Simple mode only). Looked up when
 * a gauge renders, not at import — this is module state. */
const GLOSS: Record<string, { glossKey: MessageKey }> = {
  [INDEX.sfi]: { glossKey: 'prop.spaceWx.gloss.sfi' },
  [INDEX.kp]: { glossKey: 'prop.spaceWx.gloss.kp' },
  [INDEX.a]: { glossKey: 'prop.spaceWx.gloss.a' },
  [INDEX.xray]: { glossKey: 'prop.spaceWx.gloss.xray' },
}

function Gauge({
  label,
  value,
  impact,
  dim,
  gloss,
}: {
  label: string
  value: string
  impact: Impact
  /** No severity to show: the reading is old or missing, so the bar stays empty and the line is
   *  dim — an interpretation in green or amber would read as the field now. */
  dim?: boolean
  gloss?: boolean
}) {
  const entry = gloss ? GLOSS[label] : undefined
  const def = entry ? t(entry.glossKey) : undefined
  const key = def ? (
    <Tooltip content={def} side="top">
      <span className="swx-k gloss" tabIndex={0}>
        {label}
      </span>
    </Tooltip>
  ) : (
    <span className="swx-k">{label}</span>
  )
  return (
    <div className="swx-gauge">
      <div className="swx-head">
        {key}
        <span className="swx-v">{value}</span>
      </div>
      <div className="swx-bar" aria-hidden="true">
        <span className="swx-bar-fill" style={{ background: dim ? 'transparent' : SEV_VAR[impact.sev] }} />
      </div>
      <div className="swx-impact" style={{ color: dim ? 'var(--text-dim)' : SEV_VAR[impact.sev] }}>
        {impact.text}
      </div>
    </div>
  )
}

/** "45m ago" / "3h ago", as the openings strip says it. */
function agoLabel(secs: number): string {
  const m = Math.round(secs / 60)
  return m < 60 ? t('prop.opening.ago.mins', { mins: m }) : t('prop.opening.ago.hours', { hours: Math.round(m / 60) })
}

/** The Bz gauge, honest about its reading. The station keeps its last good solar-wind sample while
 * NOAA's DSCOVR feed is unreachable, so the sample carries the time its reading was made: past
 * SOLAR_WIND_STALE_SECS the gauge says when that was instead of interpreting it, and with no sample
 * at all it says so. An older station sends no time; its sample reads as it always did. */
function BzGauge({ sw, gloss }: { sw: SolarWind | null; gloss?: boolean }) {
  if (!sw) {
    return <Gauge label={INDEX.bz} value={NO_VALUE} impact={{ sev: 'quiet', text: t('prop.spaceWx.bz.none') }} dim gloss={gloss} />
  }
  const age = solarWindAgeSecs(sw, Date.now())
  const stale = age != null && age >= SOLAR_WIND_STALE_SECS
  return (
    <Gauge
      label={INDEX.bz}
      value={sw.bzNt.toFixed(1)}
      impact={stale ? { sev: 'quiet', text: t('prop.spaceWx.bz.stale', { ago: agoLabel(age) }) } : bzImpact(sw.bzNt)}
      dim={stale}
      gloss={gloss}
    />
  )
}

export function SpaceWxGauges({ wx, gloss }: { wx: SpaceWxView; gloss?: boolean }) {
  const body = (
    <section className="swx-strip panel" aria-label={t('prop.spaceWx.aria')}>
      <Gauge
        label={INDEX.sfi}
        value={wx.sfi.toFixed(0)}
        impact={sfiImpact(wx.sfi)}
        gloss={gloss}
      />
      <Gauge
        label={INDEX.kp}
        value={wx.kp.toFixed(0)}
        impact={kpImpact(wx.kp)}
        gloss={gloss}
      />
      <Gauge
        label={INDEX.a}
        value={wx.aIndex.toFixed(0)}
        impact={aImpact(wx.aIndex)}
        gloss={gloss}
      />
      <Gauge
        label={INDEX.xray}
        value={wx.xrayClass.replace('-class', '')}
        impact={xrayImpact(wx.xrayClass)}
        gloss={gloss}
      />
      <BzGauge sw={wx.solarWind ?? null} gloss={gloss} />
    </section>
  )
  // The tooltip primitive needs a provider in scope; only mount it when glossing.
  return gloss ? <TooltipProvider>{body}</TooltipProvider> : body
}
