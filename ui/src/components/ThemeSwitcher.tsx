// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The card WORDS come
// from the catalog; the theme id is the persisted token and stays here.
import { useId } from 'react'
import { t, type MessageKey } from '../i18n'
import type { ThemeChoice } from '../useTheme'

interface Props {
  /** The operator's pick, 'system' included — not the resolved theme the page paints. */
  theme: ThemeChoice
  onChange: (t: ThemeChoice) => void
}

// The id is the VALUE (persisted, matched in CSS); the label, line and tooltip are prose and
// resolve when they are read — see `features/needVisuals.ts` for why a module-level table
// must not look its words up at import time.
const OPTIONS: { id: ThemeChoice; labelKey: MessageKey; lineKey: MessageKey; titleKey: MessageKey }[] = [
  { id: 'light', labelKey: 'theme.light.label', lineKey: 'theme.light.line', titleKey: 'theme.light.title' },
  { id: 'dark', labelKey: 'theme.dark.label', lineKey: 'theme.dark.line', titleKey: 'theme.dark.title' },
  { id: 'system', labelKey: 'theme.system.label', lineKey: 'theme.system.line', titleKey: 'theme.system.title' },
]

/** The theme as three CARDS (Settings ▸ Appearance ▸ Theme): the name, and under it a one-line
 *  personality. A card's accessible name is the name alone and its line is its description, so
 *  a screen reader says "Dark, toggle button, pressed" and then the line, not one run-on string. */
export function ThemeSwitcher({ theme, onChange }: Props) {
  const id = useId()
  return (
    <div className="theme-cards" role="group" aria-label={t('theme.aria')}>
      {OPTIONS.map((o) => (
        <button
          key={o.id}
          type="button"
          title={t(o.titleKey)}
          aria-pressed={theme === o.id}
          aria-labelledby={`${id}-${o.id}-name`}
          aria-describedby={`${id}-${o.id}-line`}
          className={`theme-card${theme === o.id ? ' active' : ''}`}
          onClick={() => onChange(o.id)}
        >
          <span className="theme-card-name" id={`${id}-${o.id}-name`}>
            {t(o.labelKey)}
          </span>
          <span className="theme-card-line" id={`${id}-${o.id}-line`}>
            {t(o.lineKey)}
          </span>
        </button>
      ))}
    </div>
  )
}
