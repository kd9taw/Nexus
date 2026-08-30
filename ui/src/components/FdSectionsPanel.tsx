// THE COCKPIT'S SECTIONS BOARD — the shipped 83-section checklist, wrapped in the two things
// an operating screen needs that the dashboard's copy does not.
//
//   WHOSE SECTIONS. `FieldDayStatus.workedSections` is THIS POSITION'S log and nothing else —
//   the club DTO carries a sections COUNT, not the identities — so the board is labelled
//   "this position" rather than left to be read as the club's progress (operator ruling Q1:
//   ship v1 position-local, no sync-protocol change). A club-wide checklist is a protocol
//   change on both ends and is deferred, not forgotten.
//
//   THE VERDICT ON THE SECTION BEING TYPED. As the exchange settles, the section in the entry
//   field is either one already in the log or a NEW MULTIPLIER — the single fact that decides
//   whether a marginal contact is worth working through the noise. It renders as a chip above
//   the board rather than as a tint on the cell, because `SectionsBoard` is reused VERBATIM
//   from FieldDayView (this landing adds one word to that file — `export` — and nothing else);
//   the chip is the whole statement, in the place the eye is already on.
//
// A bogus or half-typed section shows NOTHING. Claiming "NEW MULT" for a section that does not
// exist would promise a multiplier the commit is about to refuse — `logIt`'s exchange gate
// checks the same universe, and the two must never disagree.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Section codes are invariant
// technical tokens and stay in the code; the prose is catalog keys.
import { useMemo, type CSSProperties } from 'react'
import type { FieldDayStatus } from '../types'
import { ARRL_SECTIONS_BY_DIVISION } from '../features/arrlSections'
import { SectionsBoard } from './FieldDayView'
import { t } from '../i18n'

/** Valid ARRL/RAC section codes, derived from the canonical universe — the same set
 *  `LogEntry`'s exchange gate and the Settings picker validate against. */
const FD_SECTION_CODES = new Set(
  ARRL_SECTIONS_BY_DIVISION.flatMap((d) => d.sections).map((s) => s.code),
)

/**
 * The worked-section set for the board.
 *
 * ⚠️ MIRRORS `workedSectionSet` in FieldDayView.tsx — prefer the authoritative DTO field, fall
 * back to the log's own sections, uppercase for a case-insensitive match. Two call sites, one
 * rule; the mirror is checked in FdSectionsPanel.test.tsx rather than exported across, because
 * FieldDayView is a hot shared file this landing touches with exactly one word.
 */
export function fdWorkedSectionSet(fieldDay: FieldDayStatus | null): Set<string> {
  const set = new Set<string>()
  const src = fieldDay?.workedSections ?? (fieldDay?.log ?? []).map((q) => q.section)
  for (const s of src) {
    const code = s.trim().toUpperCase()
    if (code) set.add(code)
  }
  return set
}

const HEAD: CSSProperties = {
  display: 'flex',
  alignItems: 'center',
  flexWrap: 'wrap',
  gap: 8,
  padding: '0 0 6px',
}
const SCOPE: CSSProperties = {
  fontSize: 10,
  fontWeight: 700,
  letterSpacing: '0.04em',
  textTransform: 'uppercase',
  color: 'var(--text-faint)',
}
const VERDICT_BASE: CSSProperties = {
  display: 'inline-flex',
  alignItems: 'center',
  gap: 5,
  padding: '3px 10px',
  borderRadius: 'var(--radius-sm)',
  fontFamily: 'var(--font-mono)',
  fontSize: 13,
  fontWeight: 700,
  letterSpacing: '0.04em',
  whiteSpace: 'nowrap',
}
/** Worked = settled green; new = the loud one. The `fd-newmult` class is a styling hook for
 *  the cockpit stylesheet's pulse — the chip is already legible without it, so a sheet that
 *  never arrives costs an animation and never the statement. */
const VERDICT_WORKED: CSSProperties = {
  ...VERDICT_BASE,
  color: 'var(--status-confirmed)',
  background: 'color-mix(in srgb, var(--status-confirmed) 16%, transparent)',
  border: '1px solid color-mix(in srgb, var(--status-confirmed) 55%, transparent)',
}
const VERDICT_NEW: CSSProperties = {
  ...VERDICT_BASE,
  color: 'var(--status-new-entity)',
  background: 'color-mix(in srgb, var(--status-new-entity) 20%, transparent)',
  border: '1px solid var(--status-new-entity)',
}

export function FdSectionsPanel({
  fieldDay,
  draftSection = '',
}: {
  fieldDay: FieldDayStatus | null
  /** The section in the entry field right now. Blank, partial or bogus → no verdict. */
  draftSection?: string
}) {
  // GROWTH-KEYED, like the grid beside it. `fieldDay` is a fresh object on every 300 ms
  // snapshot poll, so keying on the container itself misses every render: the set is rebuilt
  // and — worse — gets a new identity, which makes `SectionsBoard`'s own memo miss and re-walk
  // all 83 sections twice a second on what is usually the oldest laptop in the club. Both
  // sources are append-only (the DTO's worked list and the FD log), so a length is a faithful
  // key for their contents.
  const workedLen = fieldDay?.workedSections?.length ?? 0
  const logLen = fieldDay?.log?.length ?? 0
  // eslint-disable-next-line react-hooks/exhaustive-deps
  const workedSet = useMemo(() => fdWorkedSectionSet(fieldDay), [workedLen, logLen])
  const code = draftSection.trim().toUpperCase()
  const known = FD_SECTION_CODES.has(code)
  const worked = known && workedSet.has(code)
  return (
    <div className="fd-sections-panel">
      <div style={HEAD}>
        <span style={SCOPE} title={t('fieldDay.cockpit.sections.scope.title')}>
          {t('fieldDay.cockpit.sections.scope')}
        </span>
        {known &&
          (worked ? (
            <span style={VERDICT_WORKED} role="status">
              {t('fieldDay.cockpit.sections.worked', { section: code })}
            </span>
          ) : (
            <span style={VERDICT_NEW} className="fd-newmult" role="status">
              {t('fieldDay.cockpit.sections.newMult', { section: code })}
            </span>
          ))}
      </div>
      <SectionsBoard workedSet={workedSet} />
    </div>
  )
}
