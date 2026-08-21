// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog; a hardcoded one fails CI. What does NOT come from it: every
// title, meaning, heritage note, gate hint, unit, rung label and personal-best value on this
// surface, which the backend (`get_journey`) writes and hands over as data.

import { useEffect, useState } from 'react'
import type {
  JourneyCollection,
  JourneyFeat,
  JourneyFirst,
  JourneyLadder,
  JourneySummary,
  JourneyTier,
} from '../types'
import { getJourney, getSettings } from '../api'
import { t } from '../i18n'
import { StateBlock } from './StateBlock'
import { shareCard } from '../features/shareCard'

/**
 * Journey — the in-app, beginner-first achievement layer (separate from the
 * official Awards tracker). It turns the operator's own log into a living sense of
 * progress: a level/XP spine, auto-detected "firsts", tiered sub-award ladders that
 * climb toward the big awards, fill-the-map collections, novel ham feats, personal
 * bests, and an opt-in weekly streak. Pure read of `get_journey` — informational
 * feedback, never a coercive carrot.
 */
export function JourneyView() {
  const [j, setJourney] = useState<JourneySummary | null>(null)
  const [err, setErr] = useState<string | null>(null)
  // Operator call for the share cards (best-effort — cards still render without).
  const [myCall, setMyCall] = useState('')

  useEffect(() => {
    let alive = true
    getJourney()
      .then((s) => alive && setJourney(s))
      .catch((e) => alive && setErr(e instanceof Error ? e.message : String(e)))
    getSettings()
      .then((s) => alive && setMyCall(s.mycall ?? ''))
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [])

  if (err)
    return (
      <div className="journey-view">
        <StateBlock kind="error" title={t('journey.load.failed.title')} detail={err} />
      </div>
    )
  if (!j)
    return (
      <div className="journey-view">
        <StateBlock
          kind="loading"
          title={t('journey.loading.title')}
          detail={t('journey.loading.detail')}
        />
      </div>
    )

  const xpPct = j.xpForLevel > 0 ? Math.min(100, (j.xpIntoLevel / j.xpForLevel) * 100) : 0
  const firstsDone = j.firsts.filter((f) => f.unlocked).length

  return (
    <div className="journey-view">
      {/* Hero: level + XP + the single most-attainable next milestone (goal-gradient) */}
      <section className="jy-hero panel">
        <div className="jy-level">
          <div
            className="jy-level-badge"
            title={t('journey.xpEarned.title', { xp: j.xp.toLocaleString() })}
          >
            <span className="jy-level-num">{j.level}</span>
            <span className="jy-level-cap">{t('journey.levelCap')}</span>
          </div>
          <div className="jy-level-bar-wrap">
            <div className="jy-level-top">
              <strong>{t('journey.level', { level: j.level })}</strong>
              <span className="jy-xp">
                {t('journey.xpToLevel', {
                  into: j.xpIntoLevel.toLocaleString(),
                  forLevel: j.xpForLevel.toLocaleString(),
                  next: j.level + 1,
                })}
              </span>
            </div>
            <div className="jy-bar">
              <div className="jy-bar-fill" style={{ width: `${xpPct}%` }} />
            </div>
            <div className="jy-hero-meta">
              <span>{t('journey.qsosLogged', { qsos: j.totalQsos.toLocaleString() })}</span>
              {j.streak.enabled && j.streak.weeks > 0 && (
                <span className="jy-streak" title={t('journey.streak.title')}>
                  {t('journey.streak', { count: j.streak.weeks })}
                  {j.streak.activeThisWeek ? '' : t('journey.streak.pending')}
                </span>
              )}
            </div>
          </div>
        </div>
        {j.nextMilestone && (
          <div className="jy-next" title={t('journey.next.title')}>
            <span className="jy-next-cap">{t('journey.next.cap')}</span>
            <strong className="jy-next-title">{j.nextMilestone.title}</strong>
            <span className="jy-next-go">
              {t('journey.next.go', {
                remaining: j.nextMilestone.remaining,
                current: j.nextMilestone.current,
                target: j.nextMilestone.target,
              })}
            </span>
          </div>
        )}
        <button
          type="button"
          className="jy-share"
          title={t('journey.share.title')}
          onClick={() =>
            shareCard({
              call: myCall || t('journey.share.anonCall'),
              headline: t('journey.level', { level: j.level }),
              sub: t('journey.share.sub', {
                qsos: j.totalQsos.toLocaleString(),
                xp: j.xp.toLocaleString(),
              }),
              footer: t('journey.share.footer'),
            })
          }
        >
          {t('journey.share.label')}
        </button>
      </section>

      {/* Annual marathon — this year's on-air race against your own best. */}
      {j.marathon && (
        <section className="jy-section">
          <div className="jy-section-head">
            <h2>{t('journey.marathon.head', { year: j.marathon.year })}</h2>
            <span className="jy-section-note">{t('journey.marathon.note')}</span>
          </div>
          <div className="jy-marathon panel">
            <span className="jy-marathon-score" title={t('journey.marathon.score.title')}>
              {j.marathon.score}
            </span>
            <span className="jy-marathon-parts">
              {t('journey.marathon.parts', {
                entities: j.marathon.entities,
                zones: j.marathon.zones,
              })}
            </span>
            {j.marathon.bestYear != null && j.marathon.bestYear !== j.marathon.year && (
              <span className="jy-marathon-best">
                {j.marathon.score > j.marathon.bestScore
                  ? t('journey.marathon.bestBeaten', {
                      score: j.marathon.bestScore,
                      year: j.marathon.bestYear,
                    })
                  : t('journey.marathon.best', {
                      score: j.marathon.bestScore,
                      year: j.marathon.bestYear,
                    })}
              </span>
            )}
            {j.marathon.bestYear === j.marathon.year && j.marathon.score > 0 && (
              <span className="jy-marathon-best">{t('journey.marathon.bestYear')}</span>
            )}
          </div>
        </section>
      )}

      {/* Firsts — the moments that kill the first-100-QSO motivational dead zone. */}
      <section className="jy-section">
        <div className="jy-section-head">
          <h2>{t('journey.firsts.head')}</h2>
          <span className="jy-count">
            {firstsDone}/{j.firsts.length}
          </span>
        </div>
        <div className="jy-firsts">
          {j.firsts.map((f) => (
            <FirstChip key={f.id} first={f} />
          ))}
        </div>
      </section>

      {/* Ladders — tiered sub-awards climbing toward the big official awards. */}
      <section className="jy-section">
        <div className="jy-section-head">
          <h2>{t('journey.ladders.head')}</h2>
          <span className="jy-section-note">{t('journey.ladders.note')}</span>
        </div>
        <div className="jy-ladders">
          {j.ladders.map((l) => (
            <LadderCard key={l.id} ladder={l} />
          ))}
        </div>
      </section>

      {/* Collections — fill-the-map boards. */}
      <section className="jy-section">
        <div className="jy-section-head">
          <h2>{t('journey.collections.head')}</h2>
        </div>
        <div className="jy-collections">
          {j.collections.map((c) => (
            <CollectionCard key={c.id} collection={c} />
          ))}
        </div>
      </section>

      {/* Feats — novel, ham-native accomplishments. */}
      <section className="jy-section">
        <div className="jy-section-head">
          <h2>{t('journey.feats.head')}</h2>
        </div>
        <div className="jy-feats">
          {j.feats.map((f) => (
            <FeatCard key={f.id} feat={f} myCall={myCall} />
          ))}
        </div>
      </section>

      {/* Personal bests — your own station records. */}
      {j.bests.length > 0 && (
        <section className="jy-section">
          <div className="jy-section-head">
            <h2>{t('journey.bests.head')}</h2>
          </div>
          <div className="jy-bests">
            {j.bests.map((b) => (
              <div className="jy-best" key={b.id}>
                <span className="jy-best-k">{b.title}</span>
                <span className="jy-best-v">{b.value}</span>
                {b.detail && <span className="jy-best-d">{b.detail}</span>}
              </div>
            ))}
          </div>
        </section>
      )}
    </div>
  )
}

function FirstChip({ first }: { first: JourneyFirst }) {
  // Unlocked: the backend's own meaning, with its heritage note under it — no prose of ours.
  const title = first.unlocked
    ? `${first.meaning}${first.heritage ? `\n\n${first.heritage}` : ''}`
    : t('journey.first.locked.title', { meaning: first.meaning })
  return (
    <div className={`jy-first${first.unlocked ? ' done' : ''}`} title={title}>
      <span className="jy-first-mark">{first.unlocked ? '✦' : '○'}</span>
      <span className="jy-first-body">
        <span className="jy-first-title">{first.title}</span>
        {first.unlocked && first.detail && <span className="jy-first-detail">{first.detail}</span>}
      </span>
    </div>
  )
}

function LadderCard({ ladder }: { ladder: JourneyLadder }) {
  const pct = ladder.max > 0 ? Math.min(100, (ladder.worked / ladder.max) * 100) : 0
  const cpct = ladder.max > 0 ? Math.min(100, (ladder.confirmed / ladder.max) * 100) : 0
  const done = ladder.worked >= ladder.max
  return (
    <div className={`jy-ladder${done ? ' complete' : ''}`} title={ladder.heritage}>
      <div className="jy-ladder-head">
        <strong>{ladder.title}</strong>
        <span className="jy-ladder-count">
          {ladder.worked}
          <span className="jy-dim"> {t('journey.ladder.worked')}</span> · {ladder.confirmed}
          <span className="jy-dim"> {t('journey.ladder.confirmed')}</span> / {ladder.max}
        </span>
      </div>
      <p className="jy-ladder-meaning">{ladder.meaning}</p>
      <div className="jy-ladder-track">
        {/* worked (outer) + confirmed (inner) fills */}
        <div className="jy-bar jy-ladder-bar">
          <div className="jy-bar-fill worked" style={{ width: `${pct}%` }} />
          <div className="jy-bar-fill confirmed" style={{ width: `${cpct}%` }} />
        </div>
        {/* rung ticks */}
        {ladder.rungs.map((r) => (
          <span
            key={r.label}
            className={`jy-rung${ladder.worked >= r.target ? ' hit' : ''} jy-tier-${r.tier}`}
            style={{ left: `${Math.min(100, (r.target / ladder.max) * 100)}%` }}
            title={`${r.label} — ${r.target}`}
          />
        ))}
      </div>
      {ladder.nextRung ? (
        <div className="jy-ladder-next">
          <span className={`jy-tier-pill jy-tier-${ladder.nextRung.tier}`}>
            {ladder.nextRung.label}
          </span>
          <span className="jy-ladder-go">
            {t('journey.ladder.toGo', { count: ladder.nextRung.target - ladder.worked })}
          </span>
        </div>
      ) : (
        <div className="jy-ladder-next">
          <span className="jy-tier-pill jy-tier-platinum">{t('journey.ladder.complete')}</span>
        </div>
      )}
    </div>
  )
}

function CollectionCard({ collection }: { collection: JourneyCollection }) {
  // Few cells (continents / band×mode) get labels; large sets render as a tight grid.
  const labelled = collection.cells.length <= 16
  const pct = collection.total > 0 ? Math.round((collection.worked / collection.total) * 100) : 0
  return (
    <div className="jy-collection">
      <div className="jy-collection-head">
        <strong>{collection.title}</strong>
        <span className="jy-collection-count">
          {collection.worked}/{collection.total} · {pct}%
        </span>
      </div>
      <p className="jy-collection-meaning">{collection.meaning}</p>
      <div className={`jy-cells${labelled ? ' labelled' : ''}`}>
        {collection.cells.map((c) => (
          <span
            key={c.key}
            className={`jy-cell${c.worked ? ' worked' : ''}${c.confirmed ? ' confirmed' : ''}`}
            title={
              c.confirmed
                ? t('journey.cell.confirmed.title', { label: c.label })
                : c.worked
                  ? t('journey.cell.worked.title', { label: c.label })
                  : t('journey.cell.needed.title', { label: c.label })
            }
          >
            {labelled ? c.label : ''}
          </span>
        ))}
      </div>
    </div>
  )
}

/** The five tier names. A switch rather than a table so each key is a literal at its use. */
function tierLabel(tier: JourneyTier): string {
  switch (tier) {
    case 'bronze':
      return t('journey.tier.bronze')
    case 'silver':
      return t('journey.tier.silver')
    case 'gold':
      return t('journey.tier.gold')
    case 'platinum':
      return t('journey.tier.platinum')
    case 'legendary':
      return t('journey.tier.legendary')
  }
}

function FeatCard({ feat, myCall }: { feat: JourneyFeat; myCall?: string }) {
  const pct = feat.target > 0 ? Math.min(100, (feat.current / feat.target) * 100) : 0
  const fmt = (n: number) => (Number.isInteger(n) ? String(n) : n.toFixed(0))
  return (
    <div
      className={`jy-feat${feat.unlocked ? ' done' : ''}${feat.gated ? ' gated' : ''} jy-tier-${feat.tier}`}
      title={feat.heritage}
    >
      <div className="jy-feat-head">
        <span className="jy-feat-mark">{feat.unlocked ? '★' : feat.gated ? '🔒' : '○'}</span>
        <strong>{feat.title}</strong>
        <span className={`jy-tier-pill jy-tier-${feat.tier}`}>{tierLabel(feat.tier)}</span>
        {feat.unlocked && (
          <button
            type="button"
            className="jy-share jy-share-sm"
            title={t('journey.share.feat.title')}
            onClick={() =>
              shareCard({
                call: myCall || t('journey.share.anonCall'),
                headline: feat.title,
                sub: feat.meaning,
                footer: t('journey.share.featFooter', { tier: tierLabel(feat.tier) }),
              })
            }
          >
            ⤴
          </button>
        )}
      </div>
      <p className="jy-feat-meaning">{feat.meaning}</p>
      {feat.gated ? (
        <p className="jy-feat-gate">{feat.gateHint}</p>
      ) : (
        <>
          <div className="jy-bar">
            <div className="jy-bar-fill" style={{ width: `${pct}%` }} />
          </div>
          <div className="jy-feat-foot">
            <span>
              {fmt(feat.current)} / {fmt(feat.target)} {feat.unit}
            </span>
            {feat.detail && <span className="jy-dim">{feat.detail}</span>}
          </div>
        </>
      )}
    </div>
  )
}
