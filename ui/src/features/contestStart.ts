// WHAT KEEPS A CONTEST FROM STARTING, AND WHAT ITS CABRILLO LOG LEAVES OUT, in the operator's words.
// Shared by the Contest screen's mode switch, the contest setup wizard and the end-of-contest checklist,
// so the three say the same sentence about the same thing.
import { t } from '../i18n'
import type { FdRulesetDto } from '../api'
import type { Settings } from '../types'
import { isFieldDay } from '../fdEvent'

/**
 * ⭐ **What keeps the picked contest from starting, if anything.** The mode switch refuses to
 * turn on while there is something, and says what it is.
 *
 * Field Day's own test first, because it is the engine's (`Engine::restore_opens_session`):
 * both events need a class (WFD calls it a category) and a section before the station enters
 * them, and naming both when both are blank says more than the session constructor, which
 * stops at the first. Every other reason is that constructor's own sentence off the preview
 * (`FdRulesetDto.problem`), which names the slot: "Your COUNTY is empty…".
 */
export type StartBlocker = { fields: ('class' | 'category' | 'section')[] } | { problem: string }

export function startBlocker(
  s: Pick<Settings, 'fdEvent' | 'fdClass' | 'fdSection'> | null,
  preview: FdRulesetDto | null | undefined,
): StartBlocker | null {
  const event = s?.fdEvent?.trim()
  if (s && isFieldDay(event)) {
    const fields: ('class' | 'category' | 'section')[] = []
    if (!(s.fdClass ?? '').trim()) fields.push(event === 'wfd' ? 'category' : 'class')
    if (!(s.fdSection ?? '').trim()) fields.push('section')
    if (fields.length > 0) return { fields }
  }
  return preview?.problem ? { problem: preview.problem } : null
}

/** The Settings section that fixes a blocker: Field Day for Field Day's class and section, the
 *  station data for any other contest's slots. Section ids, never tab names. */
export function blockerTarget(b: StartBlocker): string {
  return 'fields' in b ? 'field-day' : 'contest-station'
}

/** A Field Day exchange field by the name Settings gives it, so the sentence names the box. */
function fieldLabel(f: 'class' | 'category' | 'section'): string {
  if (f === 'class') return t('settings.fieldDay.class.label')
  if (f === 'category') return t('settings.fieldDay.category.label')
  return t('settings.fieldDay.section.label')
}

/** The Cabrillo lines a running session reads as it starts, so a declaration made while it runs
 *  reaches the file only after Contest mode is turned off and on. */
export const READ_AS_THE_SESSION_STARTS = ['CATEGORY-ASSISTED', 'CATEGORY-POWER']

/** One Cabrillo line the export would leave out, in words: the Settings control that fills it and
 *  the line itself (an invariant token). A line no control fills is named by itself. */
export function cabrilloLineWords(line: string): string {
  const control: Record<string, string> = {
    'CATEGORY-ASSISTED': t('settings.contestPick.assisted.label'),
    'CATEGORY-BAND': t('settings.contestPick.band.label'),
    'CATEGORY-POWER': t('settings.contestPick.power.label'),
    'CATEGORY-TRANSMITTER': t('settings.contestPick.transmitter.label'),
    OPERATORS: t('settings.contestPick.operators.label'),
    'ENTRY-CLASS': t('settings.contestPick.entryClass.label'),
    EMAIL: t('settings.contestPick.email.label'),
  }
  return control[line] ? `${control[line]} (${line})` : line
}

export function blockerText(b: StartBlocker, contest: string): string {
  if ('problem' in b) return b.problem
  const [first, second] = b.fields
  return second
    ? t('fieldDay.mode.needs.two', { contest, first: fieldLabel(first), second: fieldLabel(second) })
    : t('fieldDay.mode.needs.one', { contest, field: fieldLabel(first) })
}
