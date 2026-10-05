// THE TRANSMITTER ALARM, ON SCREEN UNTIL THE OPERATOR DISMISSES IT (operator ruling 2026-10-04,
// "Sticky until dismissed"). The radio loop raises one whenever Nexus's Flex client says what the
// operator must know about the transmitter: an unkey the radio did not confirm, a session lost
// during a transmission, an earlier session of ours still holding the transmitter, an unkey sent
// on a missed ping. The same words reach the CAT status, but that line is latest-wins: the next
// CAT message replaced the alarm, and it could scroll off unseen.
//
// WHERE: the cockpit header — CockpitHeader draws it for every cockpit that has one, APRS in its
// own header line — as the cockpits' existing alert box in its amber caution tone
// (`.cw-keyer-warn.caution`), its own row under the header's controls. In flow, never over
// anything: it cannot cover Stop TX, and the region under the header gives up the height. It
// reports and locks nothing: the dial, Tune, PTT and the CAT verdict are exactly as they were.
//
// A NEWER ALARM QUEUES, it never replaces the one on screen: replacing is how the CAT line lost
// them. The oldest shows first, with how many wait, so the banner is one alarm tall whatever
// happens. Dismiss clears exactly the alarm shown (by its id), never one raised since. The Remote
// shows it without the button: the dismissal is the station's.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The alarm's own words are
// the station's, as the CAT status's are, and pass through verbatim; the radio's name and the time
// are invariant tokens.
import { dismissTxAlarm } from '../api'
import { withErrorToast } from '../toast'
import { useStationControl } from '../stationAccess'
import { t } from '../i18n'
import type { AppSnapshot, TxAlarm } from '../types'

/** When an alarm was raised, as UTC HH:MM:SS: the clock a station logs by. */
const utc = (atMs: number) => new Date(atMs).toISOString().slice(11, 19)

export function TxAlarmBanner({
  alarms,
  onSnap,
}: {
  alarms?: TxAlarm[]
  /** Takes the snapshot the dismissal answers with. */
  onSnap?: (s: AppSnapshot) => void
}) {
  const control = useStationControl()
  const shown = alarms?.[0]
  if (!alarms || !shown) return null
  const time = utc(shown.atMs)
  return (
    <div className="cw-keyer-warn caution ch-txalarm" role="alert">
      <span className="ch-txalarm-text">
        <span aria-hidden="true">⚠ </span>
        {shown.radioName
          ? t('cockpit.txAlarm.lead.radio', { radio: shown.radioName, time })
          : t('cockpit.txAlarm.lead.noRadio', { time })}{' '}
        {shown.text}
      </span>
      {alarms.length > 1 && (
        <span className="ch-txalarm-queue">{t('cockpit.txAlarm.queue', { total: alarms.length })}</span>
      )}
      {control && (
        <button
          type="button"
          className="np-chip"
          title={t('cockpit.txAlarm.dismiss.title')}
          onClick={() =>
            void withErrorToast(() => dismissTxAlarm(shown.id), t('cockpit.txAlarm.dismiss.failed')).then((s) => {
              if (s) onSnap?.(s)
            })
          }
        >
          {t('common.dismiss')}
        </button>
      )}
    </div>
  )
}
