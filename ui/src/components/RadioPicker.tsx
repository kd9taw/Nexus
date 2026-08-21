// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog; the radios' own profile names are interpolated as they are.
import { Radio } from 'lucide-react'
import type { RadioLaunchInfo } from '../api'
import { t } from '../i18n'

/** First-screen launch picker for the two-radio setup: "which radio is this window?" Shown only
 *  when simultaneous-radios is enabled AND ≥2 radios are configured AND this window launched
 *  without a profile (see the backend `radio_launch_info`). Choosing a radio relaunches the
 *  window bound to it — so the operator never touches a shortcut or environment variable. A radio
 *  already open in another window is greyed out, so you can't accidentally double-drive one. */
export function RadioPicker({
  info,
  onChoose,
  onSingleRadio,
}: {
  info: RadioLaunchInfo
  onChoose: (id: number) => void
  /** Escape hatch: turn multi-radio off and run this one window with band-follow (the old way). */
  onSingleRadio: () => void
}) {
  return (
    <div
      className="radio-picker-overlay"
      role="dialog"
      aria-modal="true"
      aria-label={t('radios.picker.aria')}
    >
      <div className="radio-picker">
        <div className="radio-picker-head">
          <Radio size={22} aria-hidden="true" />
          <h1>{t('radios.picker.title')}</h1>
        </div>
        <p className="radio-picker-sub">{t('radios.picker.sub')}</p>
        <div className="radio-picker-list">
          {info.radios.map((r) => (
            <button
              key={r.id}
              type="button"
              className={`radio-picker-btn${r.inUse ? ' in-use' : ''}`}
              disabled={r.inUse}
              title={
                r.inUse
                  ? t('radios.picker.inUse.title', { name: r.name })
                  : t('radios.picker.choose.title', { name: r.name })
              }
              onClick={() => onChoose(r.id)}
            >
              <span className="radio-picker-name">{r.name}</span>
              {r.inUse && <span className="radio-picker-tag">{t('radios.picker.inUse.tag')}</span>}
            </button>
          ))}
        </div>
        <button type="button" className="radio-picker-single" onClick={onSingleRadio}>
          {t('radios.picker.single')}
        </button>
      </div>
    </div>
  )
}
