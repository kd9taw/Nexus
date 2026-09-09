// ---------------------------------------------------------------------------
// READING THE SESSION'S COMPOSING EXCHANGE.
//
// `FieldDayStatus.composing` is what the session is about to send — a VECTOR of slots,
// never a preformatted string, because a rendered session-level exchange is the thing
// three emitters got wrong by stamping it on rows it did not describe (spec §3.3).
//
// ⚠️ **Everything here describes THE SESSION and never a row.** A row's sent exchange is
// `FieldDayQso.mex`, which Rust renders from the row itself; nothing that describes a
// contact already logged may call these. The two surfaces that legitimately want the
// session's are the ones showing what is ABOUT to go on the air: the read-only sent
// display beside the entry boxes, and the one-tap exchange a cockpit offers to send.
// ---------------------------------------------------------------------------

import type { ContestFieldValue } from '../types'

/** One composing slot's raw value by slot id — `''` when the session has no such slot. */
export function composingSlot(
  composing: ContestFieldValue[] | undefined,
  key: string,
): string {
  return composing?.find((v) => v.key === key)?.raw ?? ''
}

/** The session's composing exchange as it goes on the air: the slots, in send order,
 *  space-separated, with empty slots left out. */
export function composingText(composing: ContestFieldValue[] | undefined): string {
  return (composing ?? [])
    .map((v) => v.raw)
    .filter((r) => r !== '')
    .join(' ')
}
