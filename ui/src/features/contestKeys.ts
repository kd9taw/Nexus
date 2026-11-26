// THE LOG STRIP'S WIPE (ALT+W AND CTRL+W) AND THE CONTEST STRIP'S CTRL+O (Ctrl+D, the third of
// its keys here, is `contestRemoval.ts`'s):
//
//   Alt+W or Ctrl+W, N1MM Logger+'s two wipe keys, wipe the log line — every log line, not only a
//   contest strip's — and a contest strip releases the serial number in flight; a second press
//   right after puts it back (`LogEntry` keeps the line it wiped). N1MM's own Ctrl+W cannot be
//   undone; here both keys can, at the operator's request;
//   Ctrl+O sets the operator call: it takes the caret to the operator box.
//
// THE OPERATOR BOX IS NOT THE STRIP'S. It is the Contest screen's — on the title line of its large
// log line while a contest runs (`ContestLargeEntry`), else in its scoreboard (`FieldDayScoreboard`)
// — so the strip only ASKS, by a window event, and the window that holds the strip answers: the main
// window shows its Contest screen, and the contest logger window has its own on its entry. Nothing
// here keys, stops or holds back a transmission, and asking changes no mode: the station's FT role
// is what it was.
import { useEffect, useRef } from 'react'

/** The window event a strip's Ctrl+O fires (`askForOperatorBox`). */
export const OPERATOR_BOX_ASK = 'nexus:operator-box'

type KeyChord = Pick<KeyboardEvent, 'key' | 'code' | 'altKey' | 'ctrlKey' | 'metaKey' | 'shiftKey'>

/** `letter` pressed: the letter decides, and the key's place only where the layout types no Latin
 *  letter there — a Mac's Option+W types ∑, a Cyrillic layout's W key types ц. */
const isLetter = (e: KeyChord, letter: string): boolean =>
  /^[a-z]$/i.test(e.key) ? e.key.toLowerCase() === letter : e.code === `Key${letter.toUpperCase()}`

/** Alt+W or Ctrl+W, and nothing else held: Ctrl with Alt is AltGr, which types characters on many
 *  keyboards, and Cmd+W closes a Mac's window. */
export function isWipeKey(e: KeyChord): boolean {
  return e.altKey !== e.ctrlKey && !e.metaKey && !e.shiftKey && isLetter(e, 'w')
}

/** Ctrl+O, and nothing else held. */
export function isOperatorBoxKey(e: KeyChord): boolean {
  return e.ctrlKey && !e.altKey && !e.metaKey && !e.shiftKey && isLetter(e, 'o')
}

/** Ctrl+O, from a strip: whatever holds this window's operator box answers. */
export function askForOperatorBox(): void {
  window.dispatchEvent(new Event(OPERATOR_BOX_ASK))
}

/** A window's answer to Ctrl+O: `onAsk` runs at each ask while the caller is mounted. */
export function useOperatorBoxAsk(onAsk: () => void): void {
  const ask = useRef(onAsk)
  ask.current = onAsk
  useEffect(() => {
    const answer = () => ask.current()
    window.addEventListener(OPERATOR_BOX_ASK, answer)
    return () => window.removeEventListener(OPERATOR_BOX_ASK, answer)
  }, [])
}
