import { useState } from 'react'
import { ampCommand } from '../api'
import { t } from '../i18n'
import { T } from '../i18n/T'
import type { AmpStatus } from '../types'

/**
 * The amplifier's own controls, in every cockpit header that has an amplifier behind it.
 *
 * ⛔ NONE OF THIS IS A STOP CONTROL, and it must never be added to a cockpit's stop-line
 * census. Putting an amplifier in standby does NOT end a transmission — the exciter keeps
 * keying and the drive passes straight through — so a control that looks like a kill switch
 * and is not one is worse than no control at all.
 *
 * Three properties this component exists to hold:
 *
 * 1. **Invisible unless an amplifier is configured.** `amp == null` renders nothing, which is
 *    the state of almost every station. Same rule the Connect pane follows.
 * 2. **Operate is driven by the AMPLIFIER, never by what we just sent.** `OPERATE` is a toggle
 *    with no idempotent "set" in the protocol, so a lost or duplicated frame inverts it. The
 *    chip reads `amp.operate` from the last status frame and nothing else; clicking sets no
 *    local state and the next poll (≤1 s) is what moves it.
 * 3. **Disabled while keyed.** Stepping band under drive pits relays and can take a PA out.
 *    This disabling is a COURTESY, not the protection — the poll thread refuses to send while
 *    the amplifier reports transmitting, and that is what actually guards the hardware.
 */
export function AmpStrip({ amp }: { amp: AmpStatus | null | undefined }) {
  // A command the queue refused. Shown once, cleared on the next successful click, because a
  // keystroke the operator watched themselves make and that silently vanished reads as broken.
  const [refused, setRefused] = useState(false)

  // Almost every station. No amplifier configured → this surface does not exist.
  if (amp == null) return null

  // Configured but not answering: the readings are gone and the controls would be sending into
  // the dark. The strip STAYS (so the operator can see the link is down where they expect the
  // amplifier to be) but nothing is clickable.
  const live = amp.linked
  const keyed = amp.transmitting === true
  const usable = live && !keyed

  const send = async (which: 'bandDown' | 'bandUp' | 'operate') => {
    const ok = await ampCommand(which)
    setRefused(!ok)
  }

  return (
    <div
      className="amp-strip"
      // Not `alert`: this is a standing readout, and an assertive role would interrupt a
      // screen-reader mid-over every time the wattage changed.
      role="group"
      aria-label={t('amp.strip.aria')}
    >
      <button
        type="button"
        className={`amp-op${amp.operate ? ' on' : ''}`}
        disabled={!usable}
        onClick={() => void send('operate')}
        title={
          keyed
            ? t('amp.strip.keyed.title')
            : amp.operate
              ? t('amp.strip.toStandby.title')
              : t('amp.strip.toOperate.title')
        }
      >
        {amp.operate ? <T k="amp.operate" /> : <T k="amp.standby" />}
      </button>

      <div className="amp-band">
        <button
          type="button"
          className="amp-band-step"
          disabled={!usable}
          onClick={() => void send('bandDown')}
          // A bare "◀" names nothing to a screen reader, and this one moves a kilowatt.
          aria-label={t('amp.strip.bandDown.aria')}
          title={keyed ? t('amp.strip.keyed.title') : t('amp.strip.bandDown.aria')}
        >
          ◀
        </button>
        {/* An index outside the ladder is unnamed rather than guessed — a wrong band name in
            front of a kilowatt is worse than an honest blank. */}
        <span className="amp-band-label">{amp.bandLabel ?? '—'}</span>
        <button
          type="button"
          className="amp-band-step"
          disabled={!usable}
          onClick={() => void send('bandUp')}
          aria-label={t('amp.strip.bandUp.aria')}
          title={keyed ? t('amp.strip.keyed.title') : t('amp.strip.bandUp.aria')}
        >
          ▶
        </button>
      </div>

      {/* Watts, or an em dash. Never a stale number: the backend clears every reading on the
          first missed poll, because a wattage beside a dead link is a fabricated one. */}
      <span className="amp-watts">
        {amp.outputWatts != null ? `${amp.outputWatts} W` : '—'}
      </span>

      {refused && (
        <span className="amp-strip-refused" role="status">
          <T k="amp.strip.refused" />
        </span>
      )}
    </div>
  )
}
