import { useState } from 'react'
import { ampCommand } from '../api'
import type { AmpStatus } from '../types'

/**
 * The amplifier's command rules, in ONE place, for every surface that sends the amplifier a
 * keystroke: the cockpit header's strip (AmpStrip) and the Amplifier box's Operate/Standby
 * button (prop/AmpPane). Two copies of these rules could drift apart, and the one that drifted
 * would be the one in front of a kilowatt.
 *
 * ⛔ NOTHING SENT FROM HERE IS A STOP. Putting an amplifier in standby does not end a
 * transmission — the exciter keeps keying and the drive passes straight through — so no control
 * built on this hook may join a cockpit's stop-line census or a sweep's `stopControls`.
 *
 * What the hook holds:
 *
 * - **`live`**: the amplifier is configured AND answering. A dead link sends nothing: the
 *   command would go into the dark.
 * - **`keyed`**: EITHER device reports keying. Elecraft has no amplifier transmit flag, and an
 *   SPE idle flag must never override a keyed exciter, so the radio's own state counts too.
 *   Stepping band or toggling Operate under drive pits relays and can take a PA out.
 * - **`usable`**: live and not keyed. This is a COURTESY, not the protection — the poll thread
 *   refuses to send while the amplifier reports transmitting (and checks current measurements),
 *   and that refusal is what actually guards the hardware.
 * - **`refused`**: the last command the queue refused or that failed, shown until the next one
 *   that goes through, because a keystroke the operator watched themselves make and that
 *   silently vanished reads as broken.
 *
 * And what it does NOT hold, on purpose: any notion of the amplifier's Operate state. `OPERATE`
 * is a toggle with no idempotent "set" in the protocol, so a lost or duplicated frame inverts
 * it. Every surface reads `amp.operate` from the last status frame and nothing else; sending
 * sets no local state, and the next poll (≤1 s) is what moves the control.
 */
export function useAmpControl(
  amp: AmpStatus | null | undefined,
  /** The RADIO's transmit state, used when the amplifier does not report its own. */
  radioTransmitting: boolean,
) {
  const [refused, setRefused] = useState(false)
  const live = amp?.linked === true
  const keyed = amp?.transmitting === true || radioTransmitting
  const usable = live && !keyed

  const send = async (which: 'bandDown' | 'bandUp' | 'operate') => {
    if (!usable) return
    try {
      const ok = await ampCommand(which)
      setRefused(!ok)
    } catch {
      setRefused(true)
    }
  }

  return { live, keyed, usable, refused, send }
}
