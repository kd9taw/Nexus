// ENTER SENDS MESSAGE'S PLATE, in the CW, RTTY and Phone cockpits' TX docks: the ESM switch, Run
// or S&P, and what the next Enter in the contest strip does — the key or keys it sends, or why it
// sends nothing, or why ESM steps aside (rule 4: "the ESM plate says why").
//
// A SENDER'S SETTING, NEVER A STOP. Nothing here keys or stops a transmission, so it has no pane id
// and is on no stop-line list; it sits in the dock beside the F-keys whose message it names. ONE
// LINE THAT NEVER MOVES ANYTHING: the host places it last in its row, where it takes the space left
// and never more, so a longer sentence is cut (whole in its title) and never shifts a control or
// wraps the row. In Phone that row holds PTT, and a control that moved under a held pointer would
// end the over.
import { t } from '../i18n'
import type { EsmDecision, EsmMode } from '../features/esm'
import { esmInertText, esmRefusalText } from '../features/esmWords'

/** N1MM's name for the feature, as contest operators say it: an invariant token. */
const ESM = 'ESM'

interface Props {
  /** This cockpit's switch. */
  on: boolean
  onSwitch: (on: boolean) => void
  mode: EsmMode
  /** The plate's Run/S&P toggle. */
  onToggle: () => void
  /** What the next Enter would do, or null while the strip holds nothing to judge. */
  preview: EsmDecision | null
}

/** The line beside the switch: what the next Enter does. */
function nextLine(preview: EsmDecision | null): string {
  if (!preview) return ''
  switch (preview.kind) {
    case 'send':
      return t('contest.esm.next', { keys: preview.keys.join(' + ') })
    case 'speak':
      return t('contest.esm.phone.speak')
    case 'refuse':
      return esmRefusalText(preview.refusal)
    case 'inert':
      return esmInertText(preview.why)
  }
}

export function EsmPlate({ on, onSwitch, mode, onToggle, preview }: Props) {
  const next = on ? nextLine(preview) : ''
  return (
    <span className={`esm-plate${on ? ' on' : ''}`} role="group" aria-label={t('contest.esm.switch.label')}>
      <button
        type="button"
        role="switch"
        aria-checked={on}
        aria-label={t('contest.esm.switch.label')}
        className={`esm-switch${on ? ' on' : ''}`}
        onClick={() => onSwitch(!on)}
        title={t('contest.esm.switch.title')}
      >
        {ESM}
      </button>
      {on && (
        <button type="button" className="esm-mode" onClick={onToggle} title={t('contest.esm.mode.title')}>
          {mode === 'run' ? t('contest.esm.mode.run') : t('contest.esm.mode.sp')}
        </button>
      )}
      {on && (
        <span className="esm-next" title={next ? `${next}\n${t('contest.esm.logOnly.title')}` : t('contest.esm.logOnly.title')}>
          {next}
        </span>
      )}
    </span>
  )
}
