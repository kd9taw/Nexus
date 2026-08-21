// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The frame count,
// the cap and the per-over payload are numbers on a technical budget and stay in the code;
// only the sentence around them comes from the catalog.
import { frameCount, MAX_FRAMES, PAYLOAD } from '../freetext'
import { t } from '../i18n'

interface Props {
  /** The current composer text. */
  text: string
  /** Fixed framing prefix not shown in the box (e.g. a broadcast's `DE <CALL> `). */
  prefix?: string
}

/**
 * Live capacity meter for the composer: how many T/R frames ("overs") the
 * message will take, out of the MAX_FRAMES limit. Turns amber as it fills and
 * red at the cap. Frame count is the honest limit (chunks word-wrap, so a flat
 * character count would mislead), but the tooltip surfaces the character math.
 */
export function FreetextMeter({ text, prefix = '' }: Props) {
  const trimmed = text.trim()
  const frames = trimmed ? frameCount(prefix + text) : 0
  const state = frames >= MAX_FRAMES ? 'full' : frames >= MAX_FRAMES - 2 ? 'warn' : 'ok'
  // One message, `{{count}}` picking the form — the old hand-rolled `s` cannot be written
  // in a language with more than two.
  const title = t('tempo.meter.title', {
    count: trimmed.length,
    frames,
    max: MAX_FRAMES,
    payload: PAYLOAD,
  })

  return (
    <span className={`char-meter ${state}`} title={title} aria-label={title}>
      <span className="cm-frames">{frames}/{MAX_FRAMES}</span>
      <span className="cm-unit">{t('tempo.meter.unit')}</span>
      {state === 'full' && <span className="cm-full">{t('tempo.meter.full')}</span>}
    </span>
  )
}
