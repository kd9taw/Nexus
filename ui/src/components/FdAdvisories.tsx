// The warn-only Field Day rule advisories (the operator's rule: WARN,
// NEVER remove or disable a surface; `enforcement` is a ruleset parameter and
// ships 'warn').
//
// Two advisories, both computed here from backend FACTS (the FdRulesetDto +
// FieldDayStatus.assistanceOn — the text is catalog keys, never Rust prose):
//
//   BANNED MODE — the active on-air tier is in the event's `bannedModes` (WFD
//   bans the WSJT suite; this is the first consumer the data has ever had).
//   Said in two words, "FT8 · no credit", with the whole sentence on hover
//   (`NoCreditChip`, below): in the ContestView event banner AND the Operate
//   cockpit header, through this component, and on the large log line beside
//   its mode chip.
//
//   ASSISTANCE — the ruleset restricts spotting or cluster use AND the
//   corresponding assistance source is effectively ON. Banner only
//   (`showAssistance`). The 2026 seed ships everything allowed, so this is
//   DORMANT today — pinned invisible (with a restricted-fixture positive
//   control) in FdAdvisories.test.tsx; flipping it live is a rules-data edit.
//
// PURE by design: every input is a prop, so tests drive it with fixture DTOs
// and the mounting surfaces (which own the data) stay in charge of fetching.
//   SPOTS OVER RF ONLY — the ruleset allows spotting only over amateur RF
//   (`spotsRfOnly`, Winter Field Day 2027) and its event is running: the station
//   is posting no spot over the internet, and this says why. Banner only, with
//   the assistance advisory; the event window and the clock are props.
//
// It is a passive status line — no handlers, no controls, no panel-vocabulary
// id — so it has no stop-line implications on any surface that hosts it.
import type { FdRulesetDto } from '../api'
import { contestName } from '../fdEvent'
import { t } from '../i18n'

/**
 * ⚠️ MIRROR of the backend assistance-source display labels
 * (`Settings::assistance_sources()`, settings.rs) — change neither side alone;
 * guard-tested from Rust (`the_advisory_ui_matches_real_assistance_source_labels`).
 * Cluster restriction maps to the cluster/RBN feed; a spotting restriction
 * covers both spot-consuming feeds (ARRL's glossary files PSK Reporter's
 * inbound reports under "Spotting/QSO Finding Assistance" — see
 * `pskr_evidence_active`). The CW decoder is decoding assistance, not
 * spotting, so neither flag matches it.
 */
const CLUSTER_SOURCES = ['DX cluster / RBN']
const SPOTTING_SOURCES = ['DX cluster / RBN', 'PSK Reporter needs']

/** The mode the station is on, when the contest does not credit it, and the sentence that says so;
 *  null when it does (`NoCreditChip`). */
function bannedMode(ruleset: FdRulesetDto, activeMode: string | undefined): { mode: string; text: string } | null {
  const mode = (activeMode ?? '').trim().toUpperCase()
  if (!mode || !ruleset.bannedModes.some((m) => m.toUpperCase() === mode)) return null
  return {
    mode,
    text: t('fieldDay.advisory.banned', { mode, event: contestName(ruleset.event), year: ruleset.rulesYear }),
  }
}

export function FdAdvisories({
  fdActive,
  ruleset,
  activeMode,
  assistanceOn = [],
  showAssistance = false,
  eventWindow,
  nowUnix,
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
  /** The ContestView banner shows the assistance advisory; the cockpit header
   *  hosts only the banned-mode chip. */
  showAssistance?: boolean
  /** The running-or-next event window (`FieldDayStatus.eventStartUnix`/`eventEndUnix`) and
   *  the clock to judge it by — the same window the engine's spot block judges. */
  eventWindow?: { start: number; end: number }
  nowUnix?: number
}) {
  if (!fdActive || !ruleset) return null
  // ⭐ THE CONTEST THAT IS RUNNING, by its own name. This read every id that was not `wfd` as
  // ARRL Field Day, so the first ruleset outside Field Day to ban a mode — the Illinois QSO
  // Party bans FT8 and FT4 — would have cited Field Day's rules at its operator. `contestName`
  // answers for every contest on the picker and gives back the id itself for one it does not
  // list, which is a token rather than another contest's name.
  const eventName = contestName(ruleset.event)
  const year = ruleset.rulesYear

  const lines: { key: string; text: string }[] = []
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
  // ⭐ No spot goes out over the internet while such a contest runs — PSK Reporter, the DX
  // cluster and pota.app are all refused by the station — and a refusal nobody can see the
  // reason for reads as a broken button. Receiving spots carries on, and the line says so.
  if (
    showAssistance &&
    ruleset.spotsRfOnly &&
    eventWindow &&
    nowUnix !== undefined &&
    eventWindow.start <= nowUnix &&
    nowUnix < eventWindow.end
  ) {
    lines.push({ key: 'spotsRfOnly', text: t('fieldDay.advisory.spotsRfOnly', { event: eventName }) })
  }
  return (
    <>
      {/* The banned mode is the log line's chip here too, where it was an amber line: the operator's
          pick (2026-10-12), "Same small chip with the full sentence on hover, in the Contest banner
          and the FT screen header". */}
      <NoCreditChip fdActive={fdActive} ruleset={ruleset} activeMode={activeMode} />
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

/**
 * ⭐ THE NO-CREDIT CHIP. First the large log line's (`ContestLargeEntry` passes it to its strip's top
 * row, right after the mode chip). That log line used to draw the banned-mode line above as a
 * full-width band over its boxes, which the operator found "very verbose and yellow and distracting"
 * (2026-10-12). The pick: "A small outlined chip beside CONTEST LOG / PH that reads 'FT8 · no
 * credit'; hover it for the full sentence. No yellow band." So it is the mode and two words, and the
 * line's sentence is its title. The same day, the same chip took the place of the amber line in the
 * Contest banner and the FT screen's header (`FdAdvisories`, above): "Same small chip with the full
 * sentence on hover, in the Contest banner and the FT screen header".
 *
 * Shown exactly when the contest does not credit the station's mode (`bannedMode`). Its name is its
 * own words (`aria-label`): a status takes its name from its title otherwise, and the title is left
 * to be its description, the whole sentence, as a screen reader gives it.
 */
export function NoCreditChip({
  fdActive,
  ruleset,
  activeMode,
}: {
  fdActive: boolean
  ruleset: FdRulesetDto | null | undefined
  /** The mode the station is on (`stationMode`), as for the banned-mode line. */
  activeMode?: string
}) {
  if (!fdActive || !ruleset) return null
  const banned = bannedMode(ruleset, activeMode)
  if (!banned) return null
  const words = t('fieldDay.advisory.noCredit', { mode: banned.mode })
  return (
    <span className="le-fd-nocredit" role="status" aria-label={words} title={banned.text}>
      {words}
    </span>
  )
}
