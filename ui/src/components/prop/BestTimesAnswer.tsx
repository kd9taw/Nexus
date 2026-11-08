// Best times, the answer side: the same block in the Conditions pane and in the floating card. Now
// first ("Now: try 20 m", how good and until about when), then the next best windows, then the week,
// then what it all rests on — the location basis on every answer, the model and its month, the mode,
// the power, a storm correction — and that it is a path forecast, not a promise they are on the air.
//
// Display only: no control here transmits, tunes, turns the rotator or works the station. Its
// buttons are Look up (the callbook, once, after the operator's own press), the mode chips and
// See full day, and its one field is a grid typed for the call.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): the words come from features/bestTimes.
import { useState } from 'react'
import type { BestMode, BestTimesView, SpotRow } from '../../types'
import { t } from '../../i18n'
import type { Units } from '../../units'
import { ModelBasis } from './ModelBasis'
import {
  BEST_MODES,
  basisLine,
  invitesGrid,
  invitesLookup,
  lastSpottedLine,
  modelLine,
  noWindowLine,
  nowLines,
  stateLine,
  weekLines,
  whereLine,
  windowRows,
} from '../../features/bestTimes'

export interface BestTimesAnswerProps {
  view: BestTimesView
  nowUnix: number
  units: Units
  antGainDbi?: number
  /** Windows to list (the card's three); all of them when absent. */
  cap?: number
  /** The Spots board's rows, for the one "last spotted" line. */
  spots?: readonly SpotRow[]
  mode: BestMode
  onMode: (m: BestMode) => void
  /** Ask the callbook once (absent where it cannot be asked). */
  onLookup?: () => void
  lookingUp?: boolean
  /** A line the lookup left (no location found, refused, offline). */
  lookupNote?: string | null
  /** A grid typed for the call (session only). */
  onGrid?: (grid: string) => void
  onFullDay?: () => void
}

export function BestTimesAnswer(p: BestTimesAnswerProps) {
  const v = p.view
  const none = stateLine(v)
  const now = nowLines(v)
  const where = whereLine(v, p.units)
  const basis = basisLine(v)
  const rows = windowRows(v, p.nowUnix, p.cap)
  const noWindow = noWindowLine(v)
  const spotted = v.state === 'answer' ? lastSpottedLine(v.call, p.spots) : null
  const model = modelLine(v, p.nowUnix, p.antGainDbi)
  return (
    <div className="bt-answer">
      <div className="bt-head">
        <span className="bt-call">{v.call}</span>
        <span className="bt-modes" role="group" aria-label={t('bestTimes.mode.aria')}>
          {BEST_MODES.map((m) => (
            <button key={m} type="button" className={`pd-mode${m === p.mode ? ' on' : ''}`} aria-pressed={m === p.mode} onClick={() => p.onMode(m)}>
              {m}
            </button>
          ))}
        </span>
      </div>
      {where && <div className="bt-where">{where}</div>}
      {none && <p className="bt-state">{none}</p>}
      {basis && (
        <p className={`bt-basis${v.basis?.kind === 'entity' ? ' rough' : ''}`}>
          {basis}
        </p>
      )}
      {(invitesLookup(v) || p.lookupNote) && (
        <div className="bt-lookup">
          {invitesLookup(v) && p.onLookup && (
            <button type="button" className="bt-link" onClick={p.onLookup} disabled={p.lookingUp}>
              {t('bestTimes.lookup')}
            </button>
          )}
          {p.lookupNote && <span className="bt-dim">{p.lookupNote}</span>}
        </div>
      )}
      {invitesGrid(v) && p.onGrid && <GridField onGrid={p.onGrid} />}
      {now && (
        <div className="bt-now">
          <div className="bt-now-head">{now.head}</div>
          {now.detail && <div>{now.detail}</div>}
          {now.notes.map((n) => (
            <div key={n} className="bt-dim">
              {n}
            </div>
          ))}
        </div>
      )}
      {v.state === 'answer' && (
        <div className="bt-next">
          <div className="bt-sub">{t('bestTimes.next.title')}</div>
          {rows.map((r) => (
            <div key={r.key} className="bt-row">
              <span className="bt-band">{r.band}</span>
              <span>{r.when}</span>
              <span className="bt-dim">{r.local}</span>
              <span>{r.word}</span>
              {r.tags.length > 0 && <span className="bt-dim">{r.tags.join(' · ')}</span>}
            </div>
          ))}
          {noWindow && <div className="bt-dim">{noWindow}</div>}
        </div>
      )}
      {v.state === 'answer' && (
        <div className="bt-week">
          {weekLines(v).map((l) => (
            <div key={l}>{l}</div>
          ))}
        </div>
      )}
      {spotted && <div className="bt-dim">{spotted}</div>}
      {v.state === 'answer' && (
        <div className="bt-foot">
          {v.engineFallback && <div>{t('prop.engine.fallback')}</div>}
          {model && <div>{model}</div>}
          <ModelBasis engine={v.answer?.engine} power={v.power} storm={v.storm} className="bt-dim" />
          {v.answer?.longUsed && <div className="route-lp-badge">{t('pathRoute.lpBadge')}</div>}
          <div className="bt-dim">{t('bestTimes.vhf')}</div>
          <div className="bt-promise">
            <span>{t('bestTimes.promise')}</span>
            {p.onFullDay && (
              <button type="button" className="bt-link" onClick={p.onFullDay}>
                {t('bestTimes.fullDay')}
              </button>
            )}
          </div>
        </div>
      )}
    </div>
  )
}

/** A grid typed for the call: session only, labelled "grid you typed" in the basis once used. */
function GridField({ onGrid }: { onGrid: (grid: string) => void }) {
  const [g, setG] = useState('')
  return (
    <form
      className="bt-grid"
      onSubmit={(e) => {
        e.preventDefault()
        if (g.trim()) onGrid(g.trim().toUpperCase())
      }}
    >
      <label>
        <span className="bt-dim">{t('bestTimes.grid.label')}</span>{' '}
        <input value={g} onChange={(e) => setG(e.target.value)} size={8} spellCheck={false} autoComplete="off" />
      </label>{' '}
      <button type="submit" className="bt-link">
        {t('bestTimes.grid.use')}
      </button>
    </form>
  )
}
