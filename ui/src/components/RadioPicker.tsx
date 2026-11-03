// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every operator-visible
// string comes from the catalog; the radios' own profile names are interpolated as they are.
import { isRadioRefusal, localizeRefusal } from '../radioRefusal'
import { useEffect, useRef, useState } from 'react'
import { Radio } from 'lucide-react'
import type { RadioLaunchInfo } from '../api'
import { t } from '../i18n'

/** First-screen launch picker for the two-radio setup: "which radio is this window?" Shown only
 *  when simultaneous-radios is enabled AND ≥2 radios are configured AND this window launched
 *  without a profile (see the backend `radio_launch_info`). Choosing a radio relaunches the
 *  window bound to it — so the operator never touches a shortcut or environment variable. A radio
 *  already open in another window is greyed out, so you can't accidentally double-drive one.
 *
 *  Nobody answering within `info.autoSeconds` repeats the operator's last answer (`onTimeout`), so
 *  a launch at sign-in, or one Remote waits on, becomes a station without a click. The countdown is
 *  on screen, and any click or key press anywhere cancels it: someone is here to choose. */
export function RadioPicker({
  info,
  onChoose,
  onSingleRadio,
  onTimeout,
  error,
}: {
  info: RadioLaunchInfo
  onChoose: (id: number) => void
  /** Escape hatch: turn multi-radio off and run this one window with band-follow (the old way). */
  onSingleRadio: () => void
  /** Nobody answered in time: repeat the last answer. */
  onTimeout: () => void
  /** Why an answer could not be carried out (the radio is open in another window), or null. */
  error: string | null
}) {
  // Seconds left, or null once someone has clicked or pressed a key (or the time ran out).
  const [left, setLeft] = useState<number | null>(info.autoSeconds > 0 ? info.autoSeconds : null)
  const timeout = useRef(onTimeout)
  timeout.current = onTimeout

  useEffect(() => {
    const cancel = () => setLeft(null)
    window.addEventListener('pointerdown', cancel, true)
    window.addEventListener('keydown', cancel, true)
    return () => {
      window.removeEventListener('pointerdown', cancel, true)
      window.removeEventListener('keydown', cancel, true)
    }
  }, [])

  useEffect(() => {
    if (left === null) return
    if (left <= 0) {
      setLeft(null) // once
      timeout.current()
      return
    }
    const id = window.setTimeout(() => setLeft((n) => (n === null ? null : n - 1)), 1000)
    return () => window.clearTimeout(id)
  }, [left])

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
        {left !== null && (
          <p className="radio-picker-auto" role="status">
            {info.autoChoice === 'windows'
              ? t('radios.picker.auto.windows', { seconds: String(left) })
              : t('radios.picker.auto.one', { seconds: String(left) })}
          </p>
        )}
        {error && (
          <p className="radio-picker-error" role="alert">
            {isRadioRefusal(error) ? localizeRefusal(error) : t('radios.picker.failed', { reason: error })}
          </p>
        )}
      </div>
    </div>
  )
}
