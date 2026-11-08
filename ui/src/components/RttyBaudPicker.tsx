// #488: THE RTTY BAUD, PICKED ON THE RTTY SCREEN. The header's mode badge is a read-out; this
// picker beside it saves the same setting Settings ▸ RTTY's Baud rate does (`rttyBaud`), so
// switching 45.45 and 50 no longer means a trip to Settings.
//
// ⚠️ DISABLED WHILE TRANSMITTING. The baud is the transmitter's bit clock: the host passes
// `disabled` while an over or the continuous TX latch is up, so nothing here changes it mid-over.
// It keys nothing and sends nothing to the radio.
//
// The rates are invariant figures (the Settings select's labels add prose, this compact picker
// does not), held as constants for the i18n guard.
import { t } from '../i18n'

/** The three rates Settings ▸ RTTY offers. */
const RATES = ['45.45', '50', '75'] as const

export function RttyBaudPicker({
  baud,
  disabled = false,
  onPick,
}: {
  /** The baud the RTTY engine reports it is using. */
  baud: number
  disabled?: boolean
  onPick: (baud: number) => void
}) {
  const value = String(baud)
  // A rate set some other way (a hand edit, an older build) is shown as it is, not replaced by
  // the nearest standard one.
  const options: readonly string[] = (RATES as readonly string[]).includes(value) ? RATES : [...RATES, value]
  return (
    <select
      className="theme-chip rtty-baud-pick"
      aria-label={t('rtty.header.baud.aria')}
      title={t('rtty.header.baud.title')}
      value={value}
      disabled={disabled}
      onChange={(e) => onPick(Number(e.target.value))}
    >
      {options.map((r) => (
        <option key={r} value={r}>
          {r}
        </option>
      ))}
    </select>
  )
}
