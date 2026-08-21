// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The chip WORDS come
// from the catalog; the theme id is the persisted token and stays here.
import { t, type MessageKey } from '../i18n'
import type { Theme } from '../useTheme'

interface Props {
  theme: Theme
  onChange: (t: Theme) => void
  /** FIELD MODE (outdoor/POTA): bigger type + high contrast, one tap, obviously reversible.
   *  Optional so existing render sites without the wiring keep exactly their old chips. */
}

// The id is the VALUE (persisted, matched in CSS); the label and tooltip are prose and
// resolve when they are read — see `features/needVisuals.ts` for why a module-level table
// must not look its words up at import time.
const OPTIONS: { id: Theme; labelKey: MessageKey; titleKey: MessageKey }[] = [
  { id: 'light', labelKey: 'theme.light.label', titleKey: 'theme.light.title' },
  { id: 'dark', labelKey: 'theme.dark.label', titleKey: 'theme.dark.title' },
]

export function ThemeSwitcher({ theme, onChange }: Props) {
  return (
    <div className="theme-switcher" role="group" aria-label={t('theme.aria')}>
      {OPTIONS.map((o) => (
        <button
          key={o.id}
          type="button"
          title={t(o.titleKey)}
          aria-pressed={theme === o.id}
          className={`theme-chip${theme === o.id ? ' active' : ''}`}
          onClick={() => onChange(o.id)}
        >
          {t(o.labelKey)}
        </button>
      ))}
    </div>
  )
}
