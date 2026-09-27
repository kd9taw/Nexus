// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The look NAMES come
// from the catalog (`labelKey`); the ids and the values are code.
//
// ONE-TAP LOOKS — Settings ▸ Appearance ▸ Workspace ▸ Look: Shack · Field / POTA · Night ·
// Contest · Big & clear · Touch. Operator pick of 2026-09-26: "Agent defines, you review", so this
// table IS the thing under review: each row is a record over settings that already exist, and the
// operator reads it as a table (see the report of the redesign's piece 7).
//
// A LOOK IS NOT A SETTING. Nothing stores which look was picked: the row reads the five settings
// below back on every render and names the look they match, or Custom. That is what makes a look
// impossible to drift from the screen — change any setting it owns by hand and the row stops
// naming it, because there is no remembered pick to disagree with. A look applies ONLY on a tap;
// nothing here runs on mount, on a timer or on a change elsewhere (SettingsPanel.looks.test.tsx).
//
// TWO KINDS, AND THE ORDER OF THE ROW SAYS WHICH:
//   · WHERE YOU ARE — Shack, Field / POTA, Night. Each sets up the whole screen: Field mode and
//     Night, plus text size and density back to Normal / Standard.
//   · HOW BIG — Contest, Big & clear, Touch. Each sets text size, density and high contrast, and
//     leaves Field mode and Night exactly as they are: a Field Day contest is Contest outdoors, a
//     big-text operator after dark is Big & clear with Night on.
// Every look sets density, and the three "where" looks all set Standard while the three "how big"
// looks each set their own, so no screen can read as two looks (looks.test.ts sweeps all 144).
//
// WHAT NO LOOK TOUCHES, deliberately — each is the operator's own pick, not part of a look:
//   · the theme: Night dims whichever theme is on (operator, 2026-09-26: "Dim the light theme"),
//     and Field mode's high-contrast blocks are glare-checked in both themes;
//   · UI scale and its per-window pin, and the colour-role presets (Settings ▸ Appearance ▸ Colours);
//   · the waterfall and scope palettes: a named pick, per scope, and Night already turns the
//     Auto palette amber after dark;
//   · motion: Reduce is the slow-computer switch, and a look resetting it would undo that; a look
//     reducing it would also stop the critical alert's pulse.
// And two settings a look reads more gently than it writes:
//   · NIGHT ON AUTO is a schedule, not a look. A look that turns Night off switches off a Night
//     the operator turned On by hand and leaves Auto alone; the row reads Auto as "not On". Only
//     the Night look switches Night on.
//   · FIELD / POTA KEEPS HIGH CONTRAST. Field mode lights the high-contrast colours by itself, and
//     useFieldMode.ts keeps the two keys apart so leaving the field restores the operator's own
//     standing contrast; a look that turns field mode on keeps that promise.

import type { MessageKey } from '../i18n'
import type { Density } from '../useDensity'
import type { NightChoice } from '../useNight'
import type { TextSize } from '../useTextSize'

export type LookId = 'shack' | 'field' | 'night' | 'contest' | 'bigClear' | 'touch'

/** The five settings a look is built from, as the row reads them back. */
export interface LookAxes {
  fieldMode: boolean
  night: NightChoice
  highContrast: boolean
  textSize: TextSize
  density: Density
}

/** What a look sets. An axis left out is one the look never touches. Night `'off'` means "not
 *  On": a manual On goes off, and Off and Auto both stay as they are. */
export interface LookSets {
  fieldMode?: boolean
  night?: 'on' | 'off'
  highContrast?: boolean
  textSize?: TextSize
  density?: Density
}

export interface Look {
  /** The invariant token. Never translated, never stored. */
  id: LookId
  labelKey: MessageKey
  sets: LookSets
}

/** The setters a look drives: the ones App already owns (useContrastPrefs, useNight, useTextSize,
 *  useDensity) and hands Settings. A look is handed these five and nothing else. */
export interface LookSetters {
  setFieldMode: (on: boolean) => void
  setNight: (c: NightChoice) => void
  setHighContrast: (on: boolean) => void
  setTextSize: (s: TextSize) => void
  setDensity: (d: Density) => void
}

/** The six looks, in the row's order: where you are, then how big. */
export const LOOKS: readonly Look[] = [
  {
    // The everyday shack: the app as installed.
    id: 'shack',
    labelKey: 'settings.workspace.looks.shack',
    sets: { fieldMode: false, night: 'off', highContrast: false, textSize: 'normal', density: 'standard' },
  },
  {
    // Outdoors, readable in sunlight: Field mode (high contrast plus a larger fit). High contrast
    // itself is left alone — see the header.
    id: 'field',
    labelKey: 'settings.workspace.looks.field',
    sets: { fieldMode: true, night: 'off', textSize: 'normal', density: 'standard' },
  },
  {
    // The shack after dark: Night on, the calm screen.
    id: 'night',
    labelKey: 'settings.workspace.looks.night',
    sets: { fieldMode: false, night: 'on', highContrast: false, textSize: 'normal', density: 'standard' },
  },
  {
    // Compact: the most on screen for a run.
    id: 'contest',
    labelKey: 'settings.workspace.looks.contest',
    sets: { highContrast: false, textSize: 'normal', density: 'dense' },
  },
  {
    // Larger text, high contrast and Comfortable spacing (#215's whole ask in one tap).
    id: 'bigClear',
    labelKey: 'settings.workspace.looks.bigClear',
    sets: { highContrast: true, textSize: 'larger', density: 'guided' },
  },
  {
    // Finger-sized targets for a touchscreen.
    id: 'touch',
    labelKey: 'settings.workspace.looks.touch',
    sets: { highContrast: false, textSize: 'normal', density: 'touch' },
  },
]

const nightMatches = (want: 'on' | 'off', now: NightChoice) => (want === 'on' ? now === 'on' : now !== 'on')

/** Is this look what the five settings show? Every axis it sets must match; the rest are free. */
export function isLook(look: Look, now: LookAxes): boolean {
  const s = look.sets
  return (
    (s.fieldMode === undefined || s.fieldMode === now.fieldMode) &&
    (s.night === undefined || nightMatches(s.night, now.night)) &&
    (s.highContrast === undefined || s.highContrast === now.highContrast) &&
    (s.textSize === undefined || s.textSize === now.textSize) &&
    (s.density === undefined || s.density === now.density)
  )
}

/** The look on screen, or Custom when the settings match none of them. */
export function lookOf(now: LookAxes): LookId | 'custom' {
  return LOOKS.find((l) => isLook(l, now))?.id ?? 'custom'
}

/** The settings as they will be once `look` is applied to `now` — what the tap will write. */
export function lookAxesOf(look: Look, now: LookAxes): LookAxes {
  const s = look.sets
  return {
    fieldMode: s.fieldMode ?? now.fieldMode,
    night: s.night === undefined || nightMatches(s.night, now.night) ? now.night : s.night,
    highContrast: s.highContrast ?? now.highContrast,
    textSize: s.textSize ?? now.textSize,
    density: s.density ?? now.density,
  }
}

/** Put the settings to `next`, writing only the ones that differ from `now` — the one writer a
 *  look and its Undo share. */
export function setAxes(next: LookAxes, now: LookAxes, set: LookSetters): void {
  if (next.fieldMode !== now.fieldMode) set.setFieldMode(next.fieldMode)
  if (next.night !== now.night) set.setNight(next.night)
  if (next.highContrast !== now.highContrast) set.setHighContrast(next.highContrast)
  if (next.textSize !== now.textSize) set.setTextSize(next.textSize)
  if (next.density !== now.density) set.setDensity(next.density)
}

/** Apply a look: write the settings it owns that differ, and nothing else. */
export function applyLook(look: Look, now: LookAxes, set: LookSetters): void {
  setAxes(lookAxesOf(look, now), now, set)
}
