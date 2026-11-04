// "MY PICTURE ALREADY SHOWS MY CALLSIGN", REMEMBERED PER PICTURE (SSTV's Transmit pane).
//
// The tick tells the transmitter not to draw the ID plate over a picture that already carries the
// station's call — a pre-made QSO card. It used to reset on every load so that one picture's
// affirmation could never ride onto the next, which is how an unidentified picture would go on the
// air. Operators reuse the same QSO cards over and over, and re-ticking each one every time is the
// friction, so the tick is now remembered FOR THAT PICTURE and only that one:
//
//   · KEYED BY CONTENT, never by name or path. The key is the SHA-256 of the file's bytes, so the
//     same card loaded from anywhere — the file picker, a drop, the gallery's Edit — is the same
//     picture, and a different picture saved under the same name (yesterday's card overwritten with
//     today's) is a different one and starts unticked. Change one byte and it is a new picture.
//   · NOTHING IS REMEMBERED WITHOUT A KEY. Where the browser cannot hash (no Web Crypto), every
//     picture starts unticked, as before: the failure direction is "ask again", never "assume".
//   · UNTICKING FORGETS. Untick a card and it is a new picture again the next time it is loaded.
//
// Shared by every window (a fact about a picture, not about a window), kept in this computer's
// browser storage, capped at the most recent MAX pictures.
export const SSTV_ID_IN_IMAGE_KEY = 'nexus.sstv.idInImage'
/** How many pictures are remembered; the oldest tick is forgotten first. A shack's QSO cards are a
 *  handful; the cap only bounds a stored list that nobody prunes. */
export const MAX_REMEMBERED = 200

/** The picture's key: the hex SHA-256 of its bytes, or null where this browser cannot hash. */
export async function pictureKey(file: Blob): Promise<string | null> {
  try {
    const subtle = globalThis.crypto?.subtle
    if (!subtle) return null
    const digest = await subtle.digest('SHA-256', await file.arrayBuffer())
    return Array.from(new Uint8Array(digest), (b) => b.toString(16).padStart(2, '0')).join('')
  } catch {
    return null
  }
}

function load(): string[] {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(SSTV_ID_IN_IMAGE_KEY) ?? '[]')
    return Array.isArray(v) ? v.filter((k): k is string => typeof k === 'string') : []
  } catch {
    return []
  }
}

/** Whether the operator ticked "already shows my callsign" for THIS picture. */
export function idInImageRemembered(key: string | null): boolean {
  return key != null && load().includes(key)
}

/** Remember (or forget) the tick for this picture. Without a key nothing is stored. */
export function rememberIdInImage(key: string | null, on: boolean): void {
  if (key == null) return
  const rest = load().filter((k) => k !== key)
  const next = on ? [...rest, key].slice(-MAX_REMEMBERED) : rest
  try {
    localStorage.setItem(SSTV_ID_IN_IMAGE_KEY, JSON.stringify(next))
  } catch {
    /* storage full or blocked: the tick still holds for this load, it is just not remembered */
  }
}
