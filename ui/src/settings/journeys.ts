// The first-run journeys: what the wizard's first step (`SetupWizard`, About you) branches into.
//
// Pure data and pure functions (no React, no storage), so the rules are node-testable. Nothing here
// is stored: the radio-count answer lives only in the open wizard, and the experience answer has its
// own table and writer (`settings/experience.ts`).
//
// The journeys, each offered and never forced, every step skippable:
//   - New to the hobby: Finish starts the first-contact walk (`experience.ts`, `finishDefaults`).
//   - Switching apps (Experienced): the log step brings the old log in through the import preview,
//     and shows where contacts go (the logbook services, their state and QRZ's Test). The wizard
//     uploads nothing and takes no login: logins stay in Settings ▸ Confirmations, their one home.
//   - Two or more radios: once the rig step has saved the second radio, the radio setup
//     (`RadioSetupWizard`, which radio for which band and mode) opens; its Cancel writes nothing.
//   - Portable (Home / Portable station setups): NOT BUILT. It waits for station profiles; when they
//     land it joins this file as a third question on the first step, and the wizard's Finish picks
//     the profile through the profiles' own writer, never by writing the fields here.

import { t } from '../i18n'
import type { ExperienceLevel } from '../types'

/** How many radios Nexus will run. Answered on the first step; unanswered is today's wizard. */
export type RadioCount = 'one' | 'several'

/** The radio-count answers, in the order the wizard offers them. Label and blurb resolve through
 *  getters with literal keys (the `EXPERIENCE_LEVELS` shape), so the catalog guards read them. */
export const RADIO_COUNTS: { value: RadioCount; readonly label: string; readonly blurb: string }[] = [
  {
    value: 'one',
    get label() {
      return t('setup.about.radios.one')
    },
    get blurb() {
      return t('setup.about.radios.one.blurb')
    },
  },
  {
    value: 'several',
    get label() {
      return t('setup.about.radios.several')
    },
    get blurb() {
      return t('setup.about.radios.several.blurb')
    },
  },
]

/** Whether the log step shows where contacts go (the switching-apps journey). */
export function showsServices(level: ExperienceLevel | null | undefined): boolean {
  return level === 'experienced'
}

/** Whether saving the second radio hands straight on to the radio setup. */
export function handsOffToRadioSetup(radios: RadioCount | null): boolean {
  return radios === 'several'
}
