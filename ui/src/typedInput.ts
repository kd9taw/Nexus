// A BOX THAT NORMALISES AS THE OPERATOR TYPES KEEPS THE CARET WHERE THEY ARE TYPING.
//
// The call boxes (and the grid, park, exchange and message boxes) upper-case on every keystroke:
// `onChange` hands the state `value.toUpperCase()`. React then finds the box holding "dABC" and
// the state saying "DABC", writes the state back into the box, and a browser puts the caret at the
// end of a value written that way. An operator who types the suffix first, goes Home and types the
// prefix saw the caret jump past the last letter after the first keystroke (CQ WW SSB, 2026-10).
//
// The fix is to write the normalised value into the box ourselves, inside the change, and put the
// caret back where it was: the box then already holds what the state will say, React writes
// nothing, and the caret stays. The value handed to the state is exactly what it was before, so
// lookups, the dupe check, the country label and logging see no difference.

/** The box's value put through `normalise`, written back into the box with the caret (or the
 *  selection) where the operator left it. Call it from `onChange` and give the result to the
 *  state in place of `e.target.value`. `normalise` must map a prefix to a prefix (upper-casing,
 *  dropping characters), which is what places the caret. */
export function normaliseTyped(el: HTMLInputElement | HTMLTextAreaElement, normalise: (s: string) => string): string {
  const { value, selectionStart, selectionEnd } = el
  const next = normalise(value)
  if (next === value) return next
  el.value = next
  if (selectionStart !== null && selectionEnd !== null) {
    el.setSelectionRange(normalise(value.slice(0, selectionStart)).length, normalise(value.slice(0, selectionEnd)).length)
  }
  return next
}

/** `normaliseTyped` for the common case: the box upper-cases. */
export const upperTyped = (el: HTMLInputElement): string => normaliseTyped(el, (s) => s.toUpperCase())

/** Each run of line breaks as one space. The shared comment (ADIF COMMENT) is one line of data,
 *  however many lines its box wraps it over; the private notes are the multi-line field. */
export const oneLine = (s: string): string => s.replace(/[\r\n]+/g, ' ')

/** `normaliseTyped` for a box that holds one line: a pasted line break lands as a space. */
export const oneLineTyped = (el: HTMLTextAreaElement): string => normaliseTyped(el, oneLine)
