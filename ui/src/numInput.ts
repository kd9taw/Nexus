// Operator-typed numbers → one canonical number. The INPUT edge.
//
// This is the counterpart of `units.ts`, and the two must not be confused: units.ts is the
// DISPLAY edge and is pure formatting ("nothing here touches a wire value"); this module is
// the other direction and produces nothing BUT wire values. Hence the one rule below.
//
// ⚠️ THE UNITS RULE, IN THE DIRECTION THAT MATTERS HERE. This normalises operator INPUT to a
// canonical number and never does the reverse: it does not localise, does not round, does not
// reformat, and nothing that goes to the radio is touched by anything in this file. An
// operator may TYPE `14,074`; what reaches the rig is the number 14.074, and what is rendered
// back to them stays whatever the display code already decided. A helper that also formatted
// would be the obvious "symmetry" and it would be wrong — it would put a comma into a CAT
// string or an ADIF field, where the wire format is not negotiable.
//
// # Why this exists (the Greek-Windows report, 2026-08)
//
// Most of the world writes decimals with a comma. `parseFloat('37,98')` is `37` — it stops at
// the comma and reports success. `Number('14,074')` is `NaN`. Neither is an error anyone sees.
// The consequences found in the tree when this was written:
//
//   - `AprsCockpit` beaconed the parsed latitude/longitude, so `37,98` went ON THE AIR as
//     `37` — a position error of a hundred kilometres, transmitted, with no warning;
//   - `SpotDialog` spotted the cluster on the wrong frequency;
//   - `MemoriesView` used bare `Number(v)` with no NaN guard at all, so a rejected entry
//     stored `0` rather than being refused.
//
// `FrequencyReadout` already did the right thing in one place, alone and undocumented. This
// module is that fix, made shared, so the next numeric input inherits it instead of
// re-deriving it — and so there is one place to test.
//
// # What it accepts, and what it deliberately refuses
//
// Every comma becomes a dot, then the whole string must parse as a finite number. `Number` is
// used rather than `parseFloat` ON PURPOSE: `parseFloat` accepts a valid PREFIX and discards
// the rest, which is the same failure mode in a different costume — `parseFloat('14.0.74')`
// is `14`, and `parseFloat('37,98')` is `37`. `Number` refuses the whole string, so garbage
// and genuinely ambiguous input (`1.234,56`, which could be a million or could be one) come
// back as `NaN` for the caller to reject, rather than silently becoming a number nobody meant.
//
// The empty string is `NaN`, not `0` — `Number('')` is `0`, and that is exactly how a blank or
// browser-rejected field committed as zero. A `<input type="number">` reports `''` for input
// its own locale parsing refused, so this case is not hypothetical on the machines this fixes.

/**
 * Parse a number an operator typed, accepting either decimal separator.
 *
 * Returns `NaN` for anything that is not a single finite number — empty, blank, trailing
 * units, two separators — so **every caller must guard with `Number.isFinite`** before using
 * the result. That guard is the second half of the fix: the sites this replaced stored the
 * bad value instead of refusing it.
 */
export function parseOperatorNumber(raw: string): number {
  // Replace ALL commas, not just the first: `14,0,74` must fail as a whole rather than
  // become 14.0 with the tail quietly dropped.
  const s = raw.trim().replace(/,/g, '.')
  if (s === '') return NaN
  const n = Number(s)
  return Number.isFinite(n) ? n : NaN
}
