import { t } from '../i18n'
import type { RadioStatus } from '../types'

/**
 * BESIDE TUNE AND THE ATU, while Nexus's own Flex client tunes with the radio's own carrier or runs
 * the radio's own ATU (Beta, each off until a tester's bench: `radio.flexTune`, `radio.flexAtu`).
 * Both carriers key at the radio's tune power, which Nexus shows and never writes (operator ruling,
 * 2026-10-08, "Radio's, shown, nothing written"), and if Nexus or the network fails during one only
 * the radio's own transmit timeout ends it. So the timeout is shown when the radio reports one, and
 * when it reports none (0, or nothing yet) the signed warning, word for word: Tune and the ATU still
 * start ("Warn only", the same day). When the licence refuses the radio's carrier here
 * (`flexTuneRefused`, `flexAtuRefused`) the note says so, since that control then keys nothing.
 * The ATU line follows: why the last press started no cycle, in the station's words, or else the
 * radio's own word for its cycle (`TUNE_SUCCESSFUL`, `TUNE_FAIL`, …), as data. Renders nothing on
 * every other station.
 *
 * Drawn AFTER a strip's controls, never between them: Tune, ATU and Stop TX keep their places.
 */
export function RadioTuneNote({ radio }: { radio: RadioStatus }) {
  const atu = radio.flexAtu
  const readouts = radio.flexTune ?? atu?.tune
  if (!readouts) return null
  const timeoutMs = readouts.txTimeoutMs ?? 0
  return (
    <span className={`radio-tune-note${timeoutMs > 0 ? '' : ' warn'}`} role="note">
      {readouts.powerPct != null && <span>{t('operate.strip.tune.radioPower', { pct: readouts.powerPct })}</span>}
      <span>{timeoutMs > 0 ? timeoutWords(timeoutMs) : t('operate.strip.tune.noRadioTimeout')}</span>
      {radio.flexTuneRefused && <span>{t('operate.strip.tune.radioRefused')}</span>}
      {radio.flexAtuRefused && <span>{t('operate.strip.atu.radioRefused')}</span>}
      {atu?.refused ? (
        <span>{atu.refused}</span>
      ) : (
        atu?.status && atu.status !== 'NONE' && <span>{t('operate.strip.atu.radioStatus', { status: atu.status })}</span>
      )}
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
