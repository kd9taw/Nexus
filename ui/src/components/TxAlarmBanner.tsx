// THE TRANSMITTER ALARM, ON SCREEN UNTIL THE OPERATOR DISMISSES IT (operator ruling 2026-10-04,
// "Sticky until dismissed"). The radio loop raises one whenever Nexus's Flex client says what the
// operator must know about the transmitter: an unkey the radio did not confirm, a session lost
// during a transmission, an earlier session of ours still holding the transmitter, an unkey sent
// on a missed ping. The same words reach the CAT status, but that line is latest-wins: the next
// CAT message replaced the alarm, and it could scroll off unseen.
//
// WHERE (operator pick 2026-11-07, "One line inside the strip"): ONE LINE IN THE TX STRIP'S ROW, beside
// the TX state, never a row of its own. As the cockpit header's last row, every alarm that came or went
// moved Stop TX 26 to 38 px; as a line of the strip, where the panes below were already at their floor
// the scope above shrank and Stop TX moved up instead. So the cell is ALWAYS DRAWN, alarms or none, at
// one constant minimum width (cockpit-panes.css `.ch-txalarm`): what it holds can never change where the
// strip breaks its lines, so nothing on the strip moves when an alarm comes or goes. It grows into the
// room left on its line, shortens its words with an ellipsis, and keeps the whole alarm on hover. It
// sits beside the controls, never over one. CockpitTxStrip and OperateQsoStrip place it (`alarm`).
//
// WHAT IT SHOWS, IN ORDER, ON THE ONE LINE (the top bar's lane may cut its chips, operator pick
// "Never wraps, alarms also shown by Stop", so every CRITICAL lane item is shown here as well and is
// never hidden anywhere):
//   1. the station's transmitter alarm, oldest first, with its Dismiss (✕, named "Dismiss");
//   2. the critical status-lane items (status.ts), in the lane's order;
//   3. "+N" when there are more ("1 of N" to a screen reader, as hidden text), every other entry whole on hover.
// THE FLOOR THE WORDS GET (cockpit-panes.css): beside the TX state the cell's minimum is 4.5em, the room
// that line has at 900x600 175 %, so it never takes a line of its own (a 13em floor did, and the taller sticky strip
// covered controls at 900x600 175 %). That keeps the ⚠, the ✕ and the start of the words; the cell grows
// into the line's room, which is far more at most sizes. On FT's own line (`ownLine`) the minimum is 13em:
// the name or nearly all of it. The count and the ✕ never shrink. Dismiss clears exactly the transmitter alarm shown (by its id), never one raised
// since; a lane item clears when its condition does. The Remote shows it without the button: the
// dismissal is the station's.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The alarm's own words are
// the station's, as the CAT status's are, and pass through verbatim; the radio's name and the time
// are invariant tokens.
import { useEffect, useState } from 'react'
import { dismissTxAlarm } from '../api'
import { withErrorToast } from '../toast'
import { subscribeStatus, type StatusItem } from '../status'
import { t } from '../i18n'
import type { AppSnapshot, TxAlarm } from '../types'

/** When an alarm was raised, as UTC HH:MM:SS: the clock a station logs by. */
const utc = (atMs: number) => new Date(atMs).toISOString().slice(11, 19)

/** The words of one transmitter alarm, its lead first. */
function alarmWords(a: TxAlarm): string {
  const time = utc(a.atMs)
  const lead = a.radioName
    ? t('cockpit.txAlarm.lead.radio', { radio: a.radioName, time })
    : t('cockpit.txAlarm.lead.noRadio', { time })
  return `${lead} ${a.text}`
}

/** The critical lane items, live (the lane's own store). */
export function useCriticalLane(): StatusItem[] {
  const [items, setItems] = useState<StatusItem[]>([])
  useEffect(() => subscribeStatus((all) => setItems(all.filter((i) => i.tier === 'critical'))), [])
  return items
}

export function TxAlarmBanner({
  alarms,
  onSnap,
  ownLine = false,
  besideMeters = false,
}: {
  alarms?: TxAlarm[]
  /** Takes the snapshot the dismissal answers with. */
  onSnap?: (s: AppSnapshot) => void
  /** The cell stands on a line of its own (FT's strip): the name's floor and a fixed height
   *  (cockpit-panes.css `.ch-txalarm-own`). Beside a TX state it does not. */
  ownLine?: boolean
  /** The strip draws a meters cell beside it, which takes the line's free room first
   *  (cockpit-panes.css `.ch-txalarm-yield`). CockpitTxStrip sets it; a screen never does. */
  besideMeters?: boolean
}) {
  const critical = useCriticalLane()
  const shown = alarms?.[0]
  const entries: string[] = [
    ...(alarms ?? []).map(alarmWords),
    ...critical.map((i) => (i.detail ? `${i.message} — ${i.detail}` : i.message)),
  ]
  const first = entries[0]
  const rest = entries.slice(1)
  return (
    // The always-drawn cell is no alert itself (an empty one would be a standing alert region); the words
    // are, so each alarm is announced as it arrives.
    <div className={`ch-txalarm${ownLine ? ' ch-txalarm-own' : ''}${besideMeters ? ' ch-txalarm-yield' : ''}`}>
      {first != null && (
        <span className="ch-txalarm-text" role="alert" title={first}>
          <span aria-hidden="true">⚠ </span>
          {first}
        </span>
      )}
      {rest.length > 0 && (
        // "+N", the same width in every language, so the words keep the cell's room: the localized "1 of N"
        // ("共 4 条中的第 1 条" in Chinese) left the alarm's own words 9 px at FT 900x600. The sentence is what a
        // screen reader reads (visually hidden text, `.sr-only`, out of flow: a name on a role-less span is not
        // valid ARIA, and browse mode would read "+3"), and it heads the hover, which lists every other entry.
        <span className="ch-txalarm-queue" title={[t('cockpit.txAlarm.queue', { total: entries.length }), ...rest].join('\n')}>
          <span aria-hidden="true">+{rest.length}</span>
          <span className="sr-only">{t('cockpit.txAlarm.queue', { total: entries.length })}</span>
        </span>
      )}
      {shown && (
        <button
          type="button"
          className="ch-txalarm-dismiss"
          aria-label={t('common.dismiss')}
          title={t('cockpit.txAlarm.dismiss.title')}
          onClick={() =>
            void withErrorToast(() => dismissTxAlarm(shown.id), t('cockpit.txAlarm.dismiss.failed')).then((s) => {
              if (s) onSnap?.(s)
            })
          }
        >
          ✕
        </button>
      )}
    </div>
  )
}
