// The warn-only Field Day rule advisories (design §4 — operator ruling: WARN,
// NEVER remove or disable a surface; `enforcement` is a ruleset parameter and
// ships 'warn').
//
// Two advisories, both computed here from backend FACTS (the FdRulesetDto +
// FieldDayStatus.assistanceOn — the text is catalog keys, never Rust prose):
//
//   BANNED MODE — the active on-air tier is in the event's `bannedModes` (WFD
//   bans the WSJT suite; this is the first consumer the data has ever had).
//   Renders in the FieldDayView event banner AND the Operate cockpit header.
//
//   ASSISTANCE — the ruleset restricts spotting or cluster use AND the
//   corresponding assistance source is effectively ON. Banner only
//   (`showAssistance`). The 2026 seed ships everything allowed, so this is
//   DORMANT today — pinned invisible (with a restricted-fixture positive
//   control) in FdAdvisories.test.tsx; flipping it live is a rules-data edit.
//
// PURE by design: every input is a prop, so tests drive it with fixture DTOs
// and the mounting surfaces (which own the data) stay in charge of fetching.
// It is a passive status line — no handlers, no controls, no panel-vocabulary
// id — so it has no stop-line implications on any surface that hosts it.
import type { FdRulesetDto } from '../api'
import { FD_EVENT_NAMES, type FdKind } from '../fdEvent'
import { t } from '../i18n'

/**
 * ⚠️ MIRROR of the backend assistance-source display labels
 * (`Settings::assistance_sources()`, settings.rs) — change neither side alone;
 * guard-tested from Rust (`the_advisory_ui_matches_real_assistance_source_labels`).
 * Cluster restriction maps to the cluster/RBN feed; a spotting restriction
 * covers both spot-consuming feeds (ARRL's glossary files PSK Reporter's
 * inbound reports under "Spotting/QSO Finding Assistance" — see
 * `pskr_evidence_active`). The AI CW decoder is decoding assistance, not
 * spotting, so neither flag matches it.
 */
const CLUSTER_SOURCES = ['DX cluster / RBN']
const SPOTTING_SOURCES = ['DX cluster / RBN', 'PSK Reporter needs']

export function FdAdvisories({
  fdActive,
  ruleset,
  activeMode,
  assistanceOn = [],
  showAssistance = false,
}: {
  /** The Field Day master switch — nothing renders while it's off. */
  fdActive: boolean
  /** The active event's ruleset facts (null until fetched → renders nothing). */
  ruleset: FdRulesetDto | null | undefined
  /** The active on-air tier/mode name (e.g. 'FT8') for the banned-mode chip;
   *  omit on a surface with no active mode. */
  activeMode?: string
  /** `FieldDayStatus.assistanceOn` — the live assistance sources. */
  assistanceOn?: string[]
  /** The FieldDayView banner shows the assistance advisory; the cockpit header
   *  hosts only the banned-mode chip. */
  showAssistance?: boolean
}) {
  if (!fdActive || !ruleset) return null
  const eventName = FD_EVENT_NAMES[(ruleset.event === 'wfd' ? 'wfd' : 'arrlfd') as FdKind]
  const year = ruleset.rulesYear
  const mode = (activeMode ?? '').trim().toUpperCase()

  const lines: { key: string; text: string }[] = []
  if (mode && ruleset.bannedModes.some((m) => m.toUpperCase() === mode)) {
    lines.push({
      key: 'banned',
      text: t('fieldDay.advisory.banned', { mode, event: eventName, year }),
    })
  }
  if (showAssistance) {
    const live = (want: string[]) => assistanceOn.filter((s) => want.includes(s))
    if (!ruleset.clusterAllowed) {
      const on = live(CLUSTER_SOURCES)
      if (on.length > 0) {
        lines.push({
          key: 'cluster',
          text: t('fieldDay.advisory.cluster', { event: eventName, year, sources: on.join(', ') }),
        })
      }
    }
    if (!ruleset.spottingAllowed) {
      const on = live(SPOTTING_SOURCES)
      if (on.length > 0) {
        lines.push({
          key: 'spotting',
          text: t('fieldDay.advisory.spotting', { event: eventName, year, sources: on.join(', ') }),
        })
      }
    }
  }
  if (lines.length === 0) return null
  return (
    <>
      {lines.map((l) => (
        // Ellipsizes in a tight header; the title carries the whole sentence.
        <div key={l.key} className={`fd-advisory ${l.key}`} role="status" title={l.text}>
          <span aria-hidden="true">⚠ </span>
          {l.text}
        </div>
      ))}
    </>
  )
}
