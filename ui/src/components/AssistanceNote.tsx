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
  const since = sinceUnix ? ` since ${utcHm(sinceUnix)}` : ''
  return (
    <div className={`assist-note${unassisted ? ' unassisted' : ''}${compact ? ' compact' : ''}`}>
      <div className="assist-note-head">
        <span className="assist-note-state mono">{unassisted ? 'UNASSISTED' : 'ASSISTED'}</span>
        <span className="assist-note-line">
          {unassisted ? (
            <>
              Assistance sources off{since}: AI CW decoder, DX cluster / RBN, PSK Reporter needs.
            </>
          ) : (
            <>
              AI CW decoder, DX cluster / RBN and PSK Reporter needs are supplying callsign
              identification{since}.
            </>
          )}
        </span>
        {onToggle && (
          <button
            type="button"
            className={`btn assist-note-btn${unassisted ? '' : ' primary'}`}
            onClick={() => onToggle(!unassisted)}
          >
            {unassisted ? 'End unassisted entry' : 'Declare unassisted entry'}
          </button>
        )}
      </div>
      <details className="assist-note-why">
        <summary>What this means for your contest category</summary>
        <ul>
          <li>
            <b>CQ WW</b> rule VIII.2 counts “a CW decoder, DX cluster, DX spotting Web sites …
            local or remote call sign and frequency decoding technology (e.g., CW Skimmer or
            Reverse Beacon Network)” as QSO-finding assistance. Using any of them places the entry
            in <b>Single Operator Assisted</b> (V.A.2) instead of Single Operator (V.A.1).
          </li>
          <li>
            <b>ARRL</b> contests call it spotting assistance and name “PSKReporter, Telnet, DX
            spotting websites or bulletin board systems, automated multi-channel decoders”.
            Single Operator may not use it (HCAT.1.1: “Use of spotting assistance is not
            permitted.”). <b>Single Operator Unlimited</b> may (HCAT.2.1). ARRL’s glossary
            defines a multi-channel decoder as software that “displays multiple decoded signals at
            the same time”. Nexus decodes one signal at a time, so that definition does not
            describe its CW decoder. It does describe the cluster, RBN and PSK Reporter feeds.
          </li>
          <li>
            Your <b>own radio’s decodes</b> are not assistance under either ruleset, so they keep
            feeding the Needed board in unassisted mode. Your outbound PSK Reporter uploads also
            keep running, because ARRL says “Generating spotting information for use by other
            stations is not considered to be spotting assistance.”
          </li>
          <li>
            <b>What this switch does not cover:</b> POTA and SOTA activator spots still arrive.
            Neither ruleset names those feeds, but both define assistance broadly enough to
            include them, so switch off the POTA/SOTA features by hand if you are entering a
            contest that counts them.
          </li>
          <li>
            Rules differ by contest and change between years, so{' '}
            <b>check the rules of the contest you are entering</b>. This note reports what CQ WW
            and ARRL currently publish. It is not a category ruling.
          </li>
        </ul>
        <p className="assist-note-keep">
          Your own settings are never rewritten. Ending unassisted mode restores the decoder and
          feeds exactly as you had them.
        </p>
      </details>
    </div>
  )
}
