// THE SCOPE VIEW CHIP — what the CW and Phone scopes draw on a radio whose own scope Nexus draws:
// the radio's scope (RF), the sound-card audio view (Audio), or, in CW, both stacked (Both). Shown
// in the scope head only once such a radio's scope has been seen. A display choice: it sends nothing
// to the radio, and nothing in it starts or stops a transmission.
//
// ⚠️ MIGRATED (i18n/hardcoded-strings.test.ts): every word is in the catalog under `scope.view.*`.
import { t } from '../i18n'
import type { ScopeView } from '../spectrum/rfSpan'

/** Each view's chip face and its tooltip. Literal `t()` calls, so the catalog's orphan check sees
 *  every key used; they resolve when the row renders, so a locale switch relabels the chips. */
const label = (v: ScopeView) =>
  v === 'rf' ? t('scope.view.rf.label') : v === 'audio' ? t('scope.view.audio.label') : t('scope.view.both.label')
const title = (v: ScopeView) =>
  v === 'rf' ? t('scope.view.rf.title') : v === 'audio' ? t('scope.view.audio.title') : t('scope.view.both.title')

export function ScopeViewChips({
  views,
  value,
  onChange,
}: {
  /** The views this cockpit offers, in order. */
  views: readonly ScopeView[]
  value: ScopeView
  onChange: (v: ScopeView) => void
}) {
  return (
    <div className="ph-span" role="group" aria-label={t('scope.view.aria')}>
      {views.map((v) => (
        <button
          key={v}
          type="button"
          className={`theme-chip${value === v ? ' active' : ''}`}
          aria-pressed={value === v}
          title={title(v)}
          onClick={() => onChange(v)}
        >
          {label(v)}
        </button>
      ))}
    </div>
  )
}
