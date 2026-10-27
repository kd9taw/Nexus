// The first-run wizard's one question, "How much have you done?", and what each answer changes.
//
// Pure data and pure functions (no React, no storage), so the rules are node-testable and the
// wizard, Settings ▸ Station and the Settings panel read the same table.
//
// The answer is stored as `Settings.experienceLevel` (one writer, `setExperienceLevel`). Absent =
// never asked: an existing station that updated, or a first run that skipped the wizard. Absent
// behaves exactly as every earlier build did (everything on, Settings fully open), so an update
// changes nothing on screen.
//
// The feature PROFILES stay the mechanism that hides and shows (`features/profiles.ts`, goal
// presets): the answer only picks the first one, and a later toggle in Settings ▸ Features moves
// to the custom set exactly as before. Nothing here touches the radio or the transmit latch.

import type { ProfileId } from '../features/profiles'
import type { FeatureId } from '../features/registry'
import { t } from '../i18n'
import type { ExperienceLevel } from '../types'
import { SETTINGS_SECTIONS } from './registry'

/** The three answers, in the order the wizard and the Settings select offer them. The `value` is
 *  the stored token; label and blurb are prose, resolved THROUGH GETTERS with literal keys (the
 *  `features/profiles.ts` shape), so a re-render in another language reads the new words and the
 *  catalog guards can read every key. */
export const EXPERIENCE_LEVELS: { value: ExperienceLevel; readonly label: string; readonly blurb: string }[] = [
  {
    value: 'newToHobby',
    get label() {
      return t('setup.about.newToHobby')
    },
    get blurb() {
      return t('setup.about.newToHobby.blurb')
    },
  },
  {
    value: 'newToDigital',
    get label() {
      return t('setup.about.newToDigital')
    },
    get blurb() {
      return t('setup.about.newToDigital.blurb')
    },
  },
  {
    value: 'experienced',
    get label() {
      return t('setup.about.experienced')
    },
    get blurb() {
      return t('setup.about.experienced.blurb')
    },
  },
]

/** The starting feature set an answer picks: `features.applyProfiles(profiles, extraOn)`.
 *  Not asked (`undefined`) is today's set, everything. */
export function featurePreset(level: ExperienceLevel | null | undefined): {
  profiles: ProfileId[]
  extraOn: FeatureId[]
} {
  switch (level) {
    case 'newToHobby':
      return { profiles: ['starter'], extraOn: [] }
    case 'newToDigital':
      // Someone who already works phone or CW keeps those screens on the rail.
      return { profiles: ['starter'], extraOn: ['phone', 'cw'] }
    default:
      return { profiles: ['everything'], extraOn: [] }
  }
}

/** What the wizard's last step starts with for an answer. Each is still the operator's to change
 *  there: starter channel packs pre-checked, the Getting started guide and the first-contact
 *  walk-through opened after Finish. Not asked = today's start (packs checked, nothing opened). */
export function finishDefaults(level: ExperienceLevel | null | undefined): {
  packs: boolean
  guide: boolean
  walk: boolean
} {
  switch (level) {
    case 'newToHobby':
      return { packs: true, guide: true, walk: true }
    case 'newToDigital':
      return { packs: true, guide: false, walk: false }
    case 'experienced':
      return { packs: false, guide: false, walk: false }
    default:
      return { packs: true, guide: false, walk: false }
  }
}

/** True when Settings starts with only the sections a first-hour operator needs open. */
export function foldsSettings(level: ExperienceLevel | null | undefined): boolean {
  return level === 'newToHobby' || level === 'newToDigital'
}

/** Whether a Settings section starts folded to its heading for this answer: a top-level section
 *  no first-hour operator needs. A nested one sits inside its parent and follows it; the
 *  Advanced groups keep their own disclosure. Search and deep links still open any of them. */
export function startsFolded(sectionId: string, level: ExperienceLevel | null | undefined): boolean {
  if (!foldsSettings(level)) return false
  const s = SETTINGS_SECTIONS.find((x) => x.id === sectionId)
  return !!s && !s.parent && !s.neededInHourOne
}
