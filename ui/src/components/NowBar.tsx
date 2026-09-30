// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every reading on
// this bar is DATA and stays in the code: the band and mode names, the entity, the callsign,
// the likelihood and the bearing, the backend's own advisory `reason`, and the three FEED
// NAMES (Cluster, Phone, PSKR — the services' own). What moved is the prose around them.
import type { ReactNode } from 'react'
import { Activity, PanelRight, Radio, SignalHigh, Target } from 'lucide-react'
import { t } from '../i18n'
import type { AppSnapshot, FeedHealth, FeedStatus, PropagationSnapshot } from '../types'
import type { View } from './ModeNav'
import { azimuthLabel, backendAzimuth } from '../grid'
import { useBandConditions } from '../bandConditions'

interface Props {
  snap: AppSnapshot
  prop: PropagationSnapshot | null
  /** Liveness of the background live feeds (cluster/RBN + PSK Reporter MQTT); null
   * until the first poll. Each started feed shows a status pill. */
  feedHealth: FeedHealth | null
  /** Drill-in gates: the Band chip opens Connect; the Need chip opens DXpeditions.
   * A disabled section's chip stays informative-only, never a dead link. */
  connectEnabled: boolean
  dxpedEnabled: boolean
  /** False when this session does not supply the needs feed. Absence is not "nothing needed". */
  needsAvailable?: boolean
  onNavigate: (v: View) => void
  /** Profile-driven chip emphasis (profiles.nowBarEmphasis — previously defined
   * but never wired): the emphasized chip leads the bar and gets the accent.
   * 'qso'/'openings' → Band first, 'rate' → Out first, 'needs'/'activation' →
   * Need first. Omitted = default order. */
  emphasis?: 'qso' | 'needs' | 'rate' | 'openings' | 'activation'
  /** The dashboard rail's switch for the cockpit on screen (components/DashRail): App passes it only
   *  beside an operating cockpit and only where the window can show the rail (`lg` and up), so the
   *  bar never offers a button that changes nothing. Omitted = no button. */
  rail?: { on: boolean; onToggle: () => void }
}

/** Compact relative age, e.g. "12s" / "4m" / "2h". The unit letter rides inside the message
 *  with its number, so a translation can never separate the two. */
function agoText(secs: number | null): string {
  if (secs == null) return ''
  if (secs < 60) return t('nowbar.age.secs', { secs })
  if (secs < 3600) return t('nowbar.age.mins', { mins: Math.round(secs / 60) })
  return t('nowbar.age.hours', { hours: Math.round(secs / 3600) })
}

/** A connector-liveness pill (one per started feed). Hidden when the feed isn't
 * running, so a user who never enabled a feed sees nothing for it. The states
 * separate "healthy but quiet" (connected — normal on a still band) from "can't
 * reach the server" (connecting/reconnecting) — previously both rendered as an
 * identical, broken-looking "waiting". */
function FeedPill({ name, status, detail }: { name: string; status: FeedStatus; detail?: string }) {
  if (!status.enabled) return null
  const ago = agoText(status.lastEventSecs)
  // Each state is a WHOLE sentence, not a stem with a tail glued on: "retrying" and
  // "retrying, last event 4m ago" are two statements, and a language that orders them
  // differently must be able to.
  const [cls, val, title] =
    status.state === 'live'
      ? [
          'good',
          ago ? t('nowbar.feed.live.value', { age: ago }) : t('nowbar.feed.live.value.noAge'),
          t('nowbar.feed.live.title', { name, age: ago }),
        ]
      : status.state === 'connected'
        ? ['good', t('nowbar.feed.connected.value'), t('nowbar.feed.connected.title', { name })]
        : status.state === 'connecting' || status.state === 'waiting'
          ? [
              'weak',
              t('nowbar.feed.connecting.value'),
              t('nowbar.feed.connecting.title', { name }),
            ]
          : status.state === 'reconnecting'
            ? [
                'bad',
                t('nowbar.feed.reconnecting.value'),
                ago
                  ? t('nowbar.feed.reconnecting.title.age', { name, age: ago })
                  : t('nowbar.feed.reconnecting.title', { name }),
              ]
            : status.state === 'idle'
              ? [
                  'ok',
                  t('nowbar.feed.idle.value', { age: ago }),
                  t('nowbar.feed.idle.title', { name, age: ago }),
                ]
              : // Defensive: an unknown future backend state renders visibly, not as a fake idle.
                ['weak', status.state, t('nowbar.feed.unknown.title', { name, state: status.state })]
  return (
    <span
      className={`nb-chip nb-feed ${cls}`}
      title={detail ? t('nowbar.feed.title.detail', { title, detail }) : title}
    >
      <Radio size={12} aria-hidden="true" />
      <span className="nb-k">{name}</span>
      <span className="nb-v">{val}</span>
    </span>
  )
}

/** A Now-Bar chip: a real button when `onClick` is given, else a plain status
 * span (so a chip never promises a drill-in to a disabled section). */
function NbChip({
  cls,
  title,
  onClick,
  children,
}: {
  cls: string
  title: string
  onClick?: () => void
  children: ReactNode
}) {
  if (onClick) {
    return (
      <button type="button" className={`nb-chip ${cls}`} onClick={onClick} title={title}>
        {children}
      </button>
    )
  }
  return (
    <span className={`nb-chip ${cls}`} title={title}>
      {children}
    </span>
  )
}

/**
 * The persistent **Now-Bar** — one always-visible line fusing the three
 * questions an operator actually asks, from data we already compute:
 *   • Is the band open?      → the current band's propagation report (tier).
 *   • Am I getting out?      → PSK Reporter "who heard me" (`nHearMe`).
 *   • What do I need now?     → the top workable DXpedition need.
 * It never invents a verdict: with no propagation data each chip says so, and
 * "getting out" reflects real spots of the operator (not a guess). Clicking the
 * band or need chip drills into the propagation nowcast.
 */

// THE BAND CHIP SAYS WHAT THE BAND MENU AND THE MAP'S LIST SAY: the band's condition comes from
// the one cell they draw (bandConditions.ts over propViz `bandConditionCell`), in the cell's own
// word, and the chip's class comes from the cell's colour. It used to say what the activity tier
// said: "quiet" for a band the model calls open and nobody has heard yet (the list: "Open · none
// heard"), and a closed band in the alert red. Now a band is green, amber or grey here exactly
// where it is there, and a closed band recedes. Unknown or stale data prints an ellipsis, never a
// word. Open, Marginal and Closed are TOKENS, like the band name beside them: the backend's
// English word in every language (operator, 2026-09-29), so there is no catalog entry for them.
const BAND_CLASS: Record<string, string> = { 'var(--band-open)': 'good', 'var(--band-marginal)': 'ok' }

export function NowBar({ snap, prop, feedHealth, connectEnabled, dxpedEnabled, onNavigate, emphasis, needsAvailable = true, rail }: Props) {
  const band = snap.radio.band
  const report = prop?.advisory.bands.find((b) => b.band === band) ?? null
  const condition = useBandConditions()(band)
  // Skip NotOpen cards: the chip must never advertise an unworkable slot as the
  // top need (the tracker keeps NotOpen cards for the board, filtered here).
  const need = prop?.dxpeditions.workableNow.find((c) => c.status !== 'NotOpen') ?? null
  // Tooltip only. The visible chip is one line of a status strip that already competes
  // for width at the 1024 px floor, and this bar's job is "is there something to chase",
  // not "where do I point" — the board a click away answers that on its face.
  const needAz = need ? backendAzimuth(need.bearingDeg, need.distanceKm) : null
  const needAzLabel = azimuthLabel(needAz)
  // The chip's tooltip, as TWO whole sentences rather than one with a tail: "live-confirmed"
  // is a claim about the expedition, and a language that puts it elsewhere must be able to.
  // `where` is the entity with its bearing beside it — both data.
  const needTitle = need
    ? (() => {
        const vals = {
          call: need.call,
          where: needAzLabel
            ? t('nowbar.need.where', { entity: need.entity, azimuth: needAzLabel })
            : need.entity,
          need: need.need,
          band: need.band,
          likelihood: need.likelihood,
        }
        return need.liveConfirmed
          ? t('nowbar.need.title.confirmed', vals)
          : t('nowbar.need.title', vals)
      })()
    : ''

  // Band open? No data, stale data or an offline snapshot prints an ellipsis — a glyph, not a word.
  const bandWord = condition.state === 'unknown' ? '…' : condition.word
  const bandCls = condition.state === 'unknown' ? 'weak' : (BAND_CLASS[condition.color ?? ''] ?? 'weak')

  // Getting out? — PSK Reporter spots OF me on this band.
  const hearMe = report?.nHearMe ?? 0
  const iHear = report?.nIHear ?? 0
  const outText = !report
    ? '—'
    : hearMe > 0
      ? t('nowbar.out.hearYou', { count: hearMe })
      : t('nowbar.out.none')
  const outCls = !report ? 'weak' : hearMe > 0 ? 'good' : 'weak'

  // Emphasized chip leads (see Props.emphasis).
  const lead: 'band' | 'out' | 'need' =
    emphasis === 'needs' || emphasis === 'activation'
      ? 'need'
      : emphasis === 'rate'
        ? 'out'
        : 'band'
  const order: Array<'band' | 'out' | 'need'> = [lead, ...(['band', 'out', 'need'] as const).filter((k) => k !== lead)]

  const chips: Record<'band' | 'out' | 'need', ReactNode> = {
    band: (
      <NbChip
        key="band"
        cls={bandCls}
        onClick={connectEnabled ? () => onNavigate('connect') : undefined}
        // `reason` is the backend's own sentence — interpolated as data, never translated
        // (that half of the propagation surface moves in phase 3).
        title={
          report?.reason ??
          (connectEnabled ? t('nowbar.band.title.connect') : t('nowbar.band.title.plain'))
        }
      >
        <Activity size={13} aria-hidden="true" />
        <span className="nb-k">{t('nowbar.band.label')}</span>
        <span className="nb-v">
          {band} {bandWord}
        </span>
      </NbChip>
    ),
    out: (
      <NbChip
        key="out"
        cls={outCls}
        title={
          report
            ? t('nowbar.out.title', { hear: hearMe, ihear: iHear, band })
            : t('nowbar.out.title.none')
        }
      >
        <SignalHigh size={13} aria-hidden="true" />
        <span className="nb-k">{t('nowbar.out.label')}</span>
        <span className="nb-v">{outText}</span>
      </NbChip>
    ),
    need: (
      <NbChip
        key="need"
        cls={`nb-need ${need ? 'good' : 'weak'}`}
        onClick={dxpedEnabled ? () => onNavigate('dxped') : undefined}
        title={!needsAvailable ? t('remote.viewUnavailable') : need ? needTitle : t('nowbar.need.title.none')}
      >
        <Target size={13} aria-hidden="true" />
        <span className="nb-k">{t('nowbar.need.label')}</span>
        <span className="nb-v">
          {!needsAvailable ? '—' : need
            ? t('nowbar.need.value', {
                entity: need.entity,
                band: need.band,
                likelihood: need.likelihood,
              })
            : t('nowbar.need.none')}
        </span>
      </NbChip>
    ),
  }

  return (
    <div className="now-bar" role="status" aria-label={t('nowbar.aria')}>
      <span className="nb-label">{t('nowbar.label')}</span>
      {order.map((k) => chips[k])}

      {prop && (
        <span
          className={`nb-src ${prop.source}`}
          title={t('nowbar.prop.title', { source: prop.source })}
        >
          {prop.source === 'live'
            ? t('nowbar.prop.live')
            : prop.source === 'partial'
              ? t('nowbar.prop.partial')
              : prop.source === 'cached'
                ? t('nowbar.prop.cached')
                : t('nowbar.prop.offline')}
        </span>
      )}

      {feedHealth && (
        <>
          <FeedPill name="Cluster" status={feedHealth.cluster} />
          {/* The SSB/phone source on its own — RBN keeps the Cluster pill green even when
              this is down, so "is my phone source up?" needs its own at-a-glance pill. */}
          <FeedPill
            name="Phone"
            status={feedHealth.phoneCluster}
            detail={feedHealth.phoneClusterHost ?? undefined}
          />
          <FeedPill name="PSKR" status={feedHealth.pskr} />
        </>
      )}

      {rail && (
        // The bar's last chip, so it sits over the column it opens. A toggle, not a status: it says
        // whether the rail is on, and the press is the whole of it.
        <button
          type="button"
          className="nb-chip nb-rail"
          aria-pressed={rail.on}
          onClick={rail.onToggle}
          title={rail.on ? t('nowbar.rail.on.title') : t('nowbar.rail.off.title')}
        >
          <PanelRight size={13} aria-hidden="true" />
          <span className="nb-k">{t('dashRail.title')}</span>
        </button>
      )}
    </div>
  )
}
