// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The card WORDS come
// from the catalog; the theme and built-in theme ids are the persisted tokens and stay here.
import { useId, type CSSProperties } from 'react'
import { t, type MessageKey } from '../i18n'
import { SKINS, type SkinFamily, type SkinId } from '../features/skins'
import type { ThemeChoice } from '../useTheme'

interface Props {
  /** The operator's pick, 'system' included — not the resolved theme the page paints. */
  theme: ThemeChoice
  /** The built-in theme painting now (useSkin), null for a standard one. */
  skin?: SkinId | null
  onChange: (t: ThemeChoice) => void
  /** Pick a built-in theme, or null for a standard one. Without it the gallery is the three
   *  standard cards alone. */
  onSkinChange?: (id: SkinId | null) => void
}

// The id is the VALUE (persisted, matched in CSS); the label, line and tooltip are prose and
// resolve when they are read — see `features/needVisuals.ts` for why a module-level table
// must not look its words up at import time.
const OPTIONS: { id: ThemeChoice; labelKey: MessageKey; lineKey: MessageKey; titleKey: MessageKey }[] = [
  { id: 'light', labelKey: 'theme.light.label', lineKey: 'theme.light.line', titleKey: 'theme.light.title' },
  { id: 'dark', labelKey: 'theme.dark.label', lineKey: 'theme.dark.line', titleKey: 'theme.dark.title' },
  { id: 'system', labelKey: 'theme.system.label', lineKey: 'theme.system.line', titleKey: 'theme.system.title' },
]

interface CardProps {
  /** Unique within the document: the name and line are referenced by id. */
  uid: string
  labelKey: MessageKey
  lineKey: MessageKey
  titleKey?: MessageKey
  /** A built-in theme's page, panel, accent and readout. */
  swatch?: readonly string[]
  pressed: boolean
  onPick: () => void
}

/** One card: the name, and under it a one-line personality. A card's accessible name is the name
 *  alone and its line is its description, so a screen reader says "Dark, toggle button, pressed"
 *  and then the line, not one run-on string. A built-in theme's card also shows a swatch of its
 *  colours, which says nothing a reader needs (it is hidden from one). */
function Card({ uid, labelKey, lineKey, titleKey, swatch, pressed, onPick }: CardProps) {
  return (
    <button
      type="button"
      title={titleKey && t(titleKey)}
      aria-pressed={pressed}
      aria-labelledby={`${uid}-name`}
      aria-describedby={`${uid}-line`}
      className={`theme-card${pressed ? ' active' : ''}`}
      onClick={onPick}
    >
      <span className="theme-card-name" id={`${uid}-name`}>
        {t(labelKey)}
      </span>
      <span className="theme-card-line" id={`${uid}-line`}>
        {t(lineKey)}
      </span>
      {swatch && (
        <span className="theme-card-swatch" aria-hidden="true">
          {swatch.map((c, i) => (
            <span key={i} style={{ '--chip': c } as CSSProperties} />
          ))}
        </span>
      )}
    </button>
  )
}

/** The theme as CARDS (Settings ▸ Appearance ▸ Theme): Light, Dark and System, then the built-in
 *  themes (features/skins.ts) in two groups, "Rig looks" and "Modern". Exactly one card is
 *  pressed: the built-in theme on screen, or else the standard pick. A built-in theme is picked
 *  with its base, so every light- or dark-theme rule still applies under it; Light, Dark and
 *  System clear it. */
export function ThemeSwitcher({ theme, skin = null, onChange, onSkinChange }: Props) {
  const id = useId()
  const standard = (
    <div className="theme-cards" role="group" aria-label={t('theme.aria')}>
      {OPTIONS.map((o) => (
        <Card
          key={o.id}
          uid={`${id}-${o.id}`}
          labelKey={o.labelKey}
          lineKey={o.lineKey}
          titleKey={o.titleKey}
          pressed={!skin && theme === o.id}
          onPick={() => {
            onChange(o.id)
            onSkinChange?.(null)
          }}
        />
      ))}
    </div>
  )
  if (!onSkinChange) return standard
  const family = (f: SkinFamily, name: string) => (
    <div className="theme-family">
      <span className="theme-family-name" id={`${id}-${f}`}>
        {name}
      </span>
      <div className="theme-cards" role="group" aria-labelledby={`${id}-${f}`}>
        {SKINS.filter((s) => s.family === f).map((s) => (
          <Card
            key={s.id}
            uid={`${id}-${s.id}`}
            labelKey={s.labelKey}
            lineKey={s.lineKey}
            swatch={[s.day['--bg'], s.day['--panel'], s.day['--accent'], s.day['--readout']]}
            pressed={skin === s.id}
            onPick={() => {
              onChange(s.base)
              onSkinChange(s.id)
            }}
          />
        ))}
      </div>
    </div>
  )
  return (
    <div className="theme-gallery">
      {standard}
      {family('rig', t('theme.family.rig'))}
      {family('modern', t('theme.family.modern'))}
    </div>
  )
}
