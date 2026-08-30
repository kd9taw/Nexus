// THE TWO FIELD DAY HEADER CHIPS — rate against a goal, and the compact score.
//
// Both are READINGS, not controls: nothing here transmits, nothing here can be disabled, and
// neither carries a panel-vocabulary id. They live in the cockpit header beside the stop
// controls, so the one thing they must never do is take focus or swallow a key — see the Esc
// note on the goal editor below, which is the only interactive element in this file.
//
// Styling is inline off the shared tokens (the SectionsBoard / club-chip idiom in
// FieldDayView.tsx), so a chip carries no size of its own and the header lays it out.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every count, score and rate is an
// invariant technical number and is interpolated through `t()` (which stringifies invariantly —
// never grouped, never a decimal comma); the words around them are catalog keys.
import { useState, type CSSProperties } from 'react'
import type { FieldDayQso, FieldDayStatus } from '../types'
import { t } from '../i18n'

/** The trailing window the rate is measured over. Matches `FdClubBoardRow.rate` (the host
 *  computes the club board's rate over the same hour), so the two numbers mean one thing. */
const RATE_WINDOW_SECS = 3600

/** SHARED across windows (storage-scope.test.ts): a target rate is a statement about how this
 *  operator is running the event, not about one window. */
const GOAL_KEY = 'nexus.fd.rateGoal'

function readGoal(): number | null {
  try {
    const v = Number(localStorage.getItem(GOAL_KEY))
    return Number.isFinite(v) && v > 0 ? Math.round(v) : null
  } catch {
    return null
  }
}

function writeGoal(goal: number | null): void {
  try {
    if (goal === null) localStorage.removeItem(GOAL_KEY)
    else localStorage.setItem(GOAL_KEY, String(goal))
  } catch {
    // Private mode / quota. The goal still applies for this session; losing a target rate
    // must never cost the operator the entry they just made.
  }
}

/** Contacts logged in the trailing hour. Rows with no timestamp (logged before the field
 *  existed, or not yet stamped) are not counted — a rate must never be a guess. */
export function fdTrailingRate(log: FieldDayQso[], nowUnix: number): number {
  const from = nowUnix - RATE_WINDOW_SECS
  let n = 0
  for (const q of log) if (q.whenUnix && q.whenUnix > from && q.whenUnix <= nowUnix) n++
  return n
}

const CHIP: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'baseline',
  gap: 5,
  padding: '3px 10px',
  borderRadius: 'var(--radius-sm)',
  fontSize: 12,
  fontWeight: 700,
  letterSpacing: '0.03em',
  whiteSpace: 'nowrap',
  color: 'var(--text-dim)',
  background: 'var(--bg-elev)',
  border: '1px solid var(--border-soft)',
}
const CHIP_LABEL: CSSProperties = {
  fontSize: 10,
  fontWeight: 700,
  letterSpacing: '0.06em',
  textTransform: 'uppercase',
  color: 'var(--text-faint)',
}
const NUM: CSSProperties = {
  fontFamily: 'var(--font-mono)',
  fontVariantNumeric: 'tabular-nums',
  fontSize: 15,
  fontWeight: 700,
  color: 'var(--text)',
}
const GOAL_INPUT: CSSProperties = {
  width: 56,
  padding: '2px 6px',
  fontFamily: 'var(--font-mono)',
  fontSize: 13,
  fontWeight: 700,
  color: 'var(--text)',
  background: 'var(--bg-elev-2)',
  border: '1px solid var(--border)',
  borderRadius: 'var(--radius-sm)',
}
const GOAL_BTN: CSSProperties = {
  padding: 0,
  fontSize: 12,
  fontWeight: 700,
  color: 'var(--text-dim)',
  background: 'none',
  border: 'none',
  cursor: 'pointer',
}

/**
 * Contacts in the trailing 60 minutes, and the goal the operator is running against.
 *
 * The goal is optional and blank by default — a made-up target on a first Field Day is a
 * number that means nothing, and the chip reads perfectly well without one.
 *
 * ⚠️ THE EDITOR NEVER SWALLOWS Esc. In this cockpit Esc stops the transmission, and it is
 * checked before any typing guard; an editor that called `stopPropagation()` on its own Esc
 * would make the stop key dead for as long as the field was open. So Esc closes the editor and
 * KEEPS GOING — pinned by a test, because nothing about the code makes the omission visible.
 */
export function FdRateChip({ log, nowUnix }: { log: FieldDayQso[]; nowUnix?: number }) {
  const [goal, setGoal] = useState<number | null>(readGoal)
  const [editing, setEditing] = useState(false)
  const [draft, setDraft] = useState('')

  // No timer: the chip re-renders with every 300 ms snapshot the cockpit already receives, so
  // `now` is fresh without this component owning a clock of its own.
  const now = nowUnix ?? Math.floor(Date.now() / 1000)
  const rate = fdTrailingRate(log, now)

  const open = () => {
    setDraft(goal === null ? '' : String(goal))
    setEditing(true)
  }
  const commit = () => {
    const v = Number(draft.trim())
    const next = draft.trim() !== '' && Number.isFinite(v) && v > 0 ? Math.round(v) : null
    setGoal(next)
    writeGoal(next)
    setEditing(false)
  }

  const behind = goal !== null && rate < goal
  return (
    <span
      style={CHIP}
      className="fd-rate-chip"
      title={
        goal === null
          ? t('fieldDay.cockpit.rate.title', { rate })
          : t('fieldDay.cockpit.rate.title.goal', { rate, goal })
      }
    >
      <span style={CHIP_LABEL}>{t('fieldDay.cockpit.rate.label')}</span>
      <span style={{ ...NUM, color: behind ? 'var(--status-new-band)' : 'var(--status-confirmed)' }}>
        {rate}
      </span>
      {editing ? (
        <input
          style={GOAL_INPUT}
          value={draft}
          autoFocus
          inputMode="numeric"
          aria-label={t('fieldDay.cockpit.rate.goal.aria')}
          placeholder={t('fieldDay.cockpit.rate.goal.placeholder')}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === 'Enter') commit()
            // Esc: close, and let the event keep travelling to the cockpit's TX stop.
            else if (e.key === 'Escape') setEditing(false)
          }}
        />
      ) : (
        <button
          type="button"
          style={GOAL_BTN}
          onClick={open}
          aria-label={t('fieldDay.cockpit.rate.goal.aria')}
        >
          {goal === null ? t('fieldDay.cockpit.rate.goal.set') : `▸ ${goal}`}
        </button>
      )}
    </span>
  )
}

/**
 * The compact score: contacts, sections, points (operator ruling Q2 — the full scoreboard is
 * one click away in the Field Day dashboard, and its arithmetic lives there).
 *
 * ⚠️ WFD SHOWS RAW QSO POINTS AND NOTHING ELSE. Winter Field Day scores by objectives at
 * submission; the ARRL power-multiplier-plus-bonuses total this DTO also carries is a number
 * WFD rules never produce, so printing it on the operating screen would claim a score the
 * entry cannot be submitted under. Same ruling — and same reason — as `fieldDay.score.wfd` on
 * the dashboard. Pinned by a test.
 */
export function FdScoreChip({ fieldDay }: { fieldDay: FieldDayStatus | null }) {
  const isWfd = (fieldDay?.event ?? '') === 'wfd'
  const qsos = fieldDay?.qsoCount ?? 0
  const sections = fieldDay?.sections ?? 0
  const qsoPts = fieldDay?.points ?? 0
  const points = isWfd ? qsoPts : (fieldDay?.totalScore ?? fieldDay?.poweredPoints ?? qsoPts)
  return (
    <span
      style={CHIP}
      className="fd-score-chip"
      title={isWfd ? t('fieldDay.cockpit.score.title.wfd') : t('fieldDay.cockpit.score.title')}
    >
      {isWfd
        ? t('fieldDay.cockpit.score.wfd', { qsos, sections, points })
        : t('fieldDay.cockpit.score', { qsos, sections, points })}
    </span>
  )
}
