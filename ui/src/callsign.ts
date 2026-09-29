// The base callsign for matching, the ENGINE'S rule: `tempo_core::message::base_call`, line for
// line. The engine is the one that decides whether a logged contact is the hunted activator (its
// park auto-tag is `same_call(record, hunt)`), so any place the UI asks "is this the same
// station?" has to give the engine's answer, or the strip shows a park that will not be logged
// and misses one that will. Hand-rolled splits gave other answers: `.split('/').pop()` made
// `KE7G/P` into `P`, so a portable activator never matched and any two `/P` stations did, and
// `.split('/')[0]` made a prefix form (`VE7/KE7G`) into its prefix.
//
// NOT for a QRZ page key or a DXCC lookup. Those keep their own rules and their own Rust twins
// (`qrz_url`: the longest segment; `propagation::dxcc::base_call`: the LOCATION side, because
// `KH8/W1AW` is worked from American Samoa).

/** A segment shaped like a full call: a letter somewhere after its first digit (W1AW, W9XYZ),
 *  unlike a bare prefix (KH8, VE7) or an affix (P, MM, QRP, 4). */
function looksFull(segment: string): boolean {
  const digit = segment.search(/[0-9]/)
  return digit >= 0 && /[A-Z]/.test(segment.slice(digit + 1))
}

/** The call a compound callsign is built around, uppercased: `KE7G/P` → `KE7G`,
 *  `VE7/W7ABC` → `W7ABC`, `DL/W7ABC/P` → `W7ABC`. An FT hashed call (`<W9XYZ>`) is unwrapped
 *  first, as the engine does. */
export function baseCall(call: string): string {
  const up = call.trim().replace(/^<+/, '').replace(/>+$/, '').trim().toUpperCase()
  if (!up.includes('/')) return up
  const parts = up.split('/')
  // The LAST full-looking segment: for a prefix form (KH8/W1AW, VP2E/AA9A) the home call comes
  // last; for a suffix form (W9XYZ/P, W9XYZ/4) the affix is not full-looking, so the call wins.
  for (let i = parts.length - 1; i >= 0; i--) if (looksFull(parts[i])) return parts[i]
  // None looks like a call: the longest non-empty segment, the LAST of equals (Rust's
  // `max_by_key`), or the whole string when every segment is empty.
  let longest = ''
  for (const p of parts) if (p && p.length >= longest.length) longest = p
  return longest || up
}

/** Two callsigns name the same station: `tempo_core::message::same_call`. */
export function sameCall(a: string, b: string): boolean {
  return baseCall(a) === baseCall(b)
}
