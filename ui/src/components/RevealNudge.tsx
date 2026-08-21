import { t } from '../i18n'
import { T } from '../i18n/T'
import type { FeatureDef } from '../features/registry'
import type { Achievement } from '../types'

interface Props {
  feature: FeatureDef
  achievement: Achievement
  /** Turn the suggested feature on. */
  onEnable: () => void
  /** Permanently dismiss this nudge. */
  onDismiss: () => void
}

/**
 * Adaptive-reveal nudge: a single gentle, dismissible suggestion to turn on a
 * feature the operator's activity just made relevant (e.g. worked first DX →
 * "turn on Awards"). Surfaced by `useReveals`; never auto-enables.
 */
export function RevealNudge({ feature, achievement, onEnable, onDismiss }: Props) {
  return (
    <div className="reveal-nudge" role="note">
      <span className="reveal-icon" aria-hidden>
        ✨
      </span>
      <span className="reveal-text">
        {/* One sentence, one key: the emphasis sits mid-sentence and the two values are the
            reason the sentence exists, so neither may be split off into its own string — a
            language that orders them differently could not be translated. */}
        <T
          k="reveal.prompt"
          tags={{ b: <strong /> }}
          vals={{ achievement: achievement.title, feature: feature.label }}
        />{' '}
        <span className="reveal-sub">{feature.oneLine}</span>
      </span>
      <button type="button" className="reveal-enable" onClick={onEnable}>
        {t('reveal.enable')}
      </button>
      <button type="button" className="reveal-dismiss" onClick={onDismiss}>
        {t('reveal.notNow')}
      </button>
    </div>
  )
}
