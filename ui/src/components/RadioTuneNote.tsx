import { t } from '../i18n'
import type { RadioStatus } from '../types'

/**
 * BESIDE TUNE, while Nexus's own Flex client tunes with the radio's own carrier (Beta, off until a
 * tester's bench: `radio.flexTune`). The carrier keys at the radio's tune power, which Nexus shows
 * and never writes (operator ruling, 2026-10-08, "Radio's, shown, nothing written"), and if Nexus or
 * the network fails during a tune only the radio's own transmit timeout ends it. So the timeout is
 * shown when the radio reports one, and when it reports none (0, or nothing yet) the signed warning,
 * word for word: Tune still starts ("Warn only", the same day). When the licence refuses the
 * radio's carrier here (`flexTuneRefused`) the note says so, since Tune then keys nothing. Renders
 * nothing on every other station.
 *
 * Drawn AFTER a strip's controls, never between them: Tune, ATU and Stop TX keep their places.
 */
export function RadioTuneNote({ radio }: { radio: RadioStatus }) {
  const tune = radio.flexTune
  if (!tune) return null
  const timeoutMs = tune.txTimeoutMs ?? 0
  return (
    <span className={`radio-tune-note${timeoutMs > 0 ? '' : ' warn'}`} role="note">
      {tune.powerPct != null && <span>{t('operate.strip.tune.radioPower', { pct: tune.powerPct })}</span>}
      <span>{timeoutMs > 0 ? timeoutWords(timeoutMs) : t('operate.strip.tune.noRadioTimeout')}</span>
      {radio.flexTuneRefused && <span>{t('operate.strip.tune.radioRefused')}</span>}
    </span>
  )
}

/** The radio's transmit timeout in the unit it reads best in: whole minutes from two minutes up,
 *  seconds below (a fraction to one decimal place). */
function timeoutWords(ms: number): string {
  const sec = ms / 1000
  if (sec >= 120 && sec % 60 === 0) return t('operate.strip.tune.radioTimeoutMin', { min: sec / 60 })
  return t('operate.strip.tune.radioTimeoutSec', { sec: Number.isInteger(sec) ? sec : Number(sec.toFixed(1)) })
}
