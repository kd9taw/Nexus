// The one place the contest-category wording lives, so the Settings switch and the CW
// cockpit footer can never drift apart on a rules claim.
//
// ⚠️ EVERY rule statement below is quoted or paraphrased from a primary source that was
// fetched and read (2026-07-30). Do not add a claim here that has not been. An operator may
// choose a category on the strength of this text, and a confidently wrong sentence is worse
// than no sentence:
//
//   • CQ WW rules — https://www.cqww.com/rules.htm
//     V.A.1 Single Operator: "QSO finding assistance of any kind is prohibited (see VIII.2)."
//     V.A.2 Single Operator Assisted: "Entrants in this category may use QSO finding
//     assistance (see VIII.2)."
//     VIII.2 defines it as "The use of any technology or other source that provides callsign
//     or multiplier identification of a signal to the operator. This includes, but is not
//     limited to, use of a CW decoder, DX cluster, DX spotting Web sites (e.g., DX Summit),
//     local or remote call sign and frequency decoding technology (e.g., CW Skimmer or
//     Reverse Beacon Network), or operating arrangements involving other individuals."
//
//   • ARRL International DX Contest Rules v2.0 (Definitions and Glossary v1.05, 10 Feb 2022)
//     — https://contests.arrl.org/ContestRules/DX-Rules.pdf
//     HCAT.1.1 Single Operator: "Use of spotting assistance is not permitted."
//     HCAT.2 Single Operator Unlimited (SOU), HCAT.2.1: "Use of publicly available spotting
//     assistance is permitted. Spotting information must be derived from sources within the
//     station's circle or sources open to the general public."
//     Glossary, "Spotting/QSO Finding Assistance": "Use of any technology that provides call
//     sign or multiplier identification of a signal to the operator. This includes
//     PSKReporter, Telnet, DX spotting websites or bulletin board systems, automated
//     multi-channel decoders, etc." … "Generating spotting information for use by other
//     stations is not considered to be spotting assistance."
//     Glossary, "Automated Multi-Channel Decoder": "Device such as CW Skimmer software that
//     provides information about the identity and frequency of contest station transmissions
//     while functioning independently of the operator's direct control and participation.
//     Software that displays multiple decoded signals at the same time is considered to be a
//     multi-channel decoder."
//
// The two rulesets genuinely DIFFER on the CW decoder, which is why this text does not lump
// them together: CQ WW names "a CW decoder" without qualification, while ARRL names
// *multi-channel* decoders and defines that as software showing several decoded signals at
// once. Nexus decodes one signal at a time. We state what each ruleset says and stop there —
// we never tell the operator which category they are in.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The wording now lives
// in `i18n/en.ts` under `assist.*` — including the rule quotations, which carry a ⚠️ there
// telling a translator to leave the quoted text, the rule numbers and the category names alone.

import { t } from '../i18n'
import { T } from '../i18n/T'

/** UTC HH:MMZ for a unix stamp — the stamp format the rest of the app uses in status lines. */
function utcHm(unix: number): string {
  const d = new Date(unix * 1000)
  const p = (n: number) => String(n).padStart(2, '0')
  return `${p(d.getUTCHours())}:${p(d.getUTCMinutes())}Z`
}

interface Props {
  /** Is an unassisted entry declared right now? */
  unassisted: boolean
  /** When the current posture began (unix secs), from the assistance journal. Omit to hide. */
  sinceUnix?: number | null
  /** Toggle handler. Omit to render the note without a button (e.g. a read-only footer). */
  onToggle?: (on: boolean) => void
  /** Compact single-line presentation for a cockpit status strip. */
  compact?: boolean
}

/**
 * States the contest-category implication of the assistance sources Nexus is running, and
 * offers the one switch that turns them all off.
 *
 * The headline is always a plain statement of fact about THIS station; the rule citations sit
 * in a collapsed details block so the footer stays one line for an operator who already knows.
 */
export function AssistanceNote({ unassisted, sinceUnix, onToggle, compact }: Props) {
  // The stamp is a VALUE, not a sentence fragment: each of the four postures below is one whole
  // catalog sentence, so a language that puts "since 14:02Z" first still reads correctly.
  const since = sinceUnix ? utcHm(sinceUnix) : ''
  const line = unassisted
    ? since
      ? t('assist.sources.offSince', { since })
      : t('assist.sources.off')
    : since
      ? t('assist.sources.onSince', { since })
      : t('assist.sources.on')
  return (
    <div className={`assist-note${unassisted ? ' unassisted' : ''}${compact ? ' compact' : ''}`}>
      <div className="assist-note-head">
        <span className="assist-note-state mono">
          {unassisted ? t('assist.state.unassisted') : t('assist.state.assisted')}
        </span>
        <span className="assist-note-line">{line}</span>
        {onToggle && (
          <button
            type="button"
            className={`btn assist-note-btn${unassisted ? '' : ' primary'}`}
            onClick={() => onToggle(!unassisted)}
          >
            {unassisted ? t('assist.toggle.end') : t('assist.toggle.declare')}
          </button>
        )}
      </div>
      <details className="assist-note-why">
        <summary>{t('assist.why.summary')}</summary>
        <ul>
          {/* The emphasis sits mid-sentence in every one of these, so each is ONE key with a
              `<b>` marker; the element comes from here, never from the catalog. */}
          <li>
            <T k="assist.why.cqww" tags={{ b: <b /> }} />
          </li>
          <li>
            <T k="assist.why.arrl" tags={{ b: <b /> }} />
          </li>
          <li>
            <T k="assist.why.ownDecodes" tags={{ b: <b /> }} />
          </li>
          <li>
            <T k="assist.why.notCovered" tags={{ b: <b /> }} />
          </li>
          <li>
            <T k="assist.why.checkRules" tags={{ b: <b /> }} />
          </li>
        </ul>
        <p className="assist-note-keep">{t('assist.keep')}</p>
      </details>
    </div>
  )
}
