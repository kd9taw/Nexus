// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts): every word comes from
// the catalog; the role and preset ids are the stored tokens.
//
// Settings ▸ Appearance ▸ Colours — one row per colour role (features/paletteRoles.ts): the role's
// name, a chip per pre-checked preset, a Reset that is greyed while the role is on its default,
// and the plain-language line saying what the colour paints. No hex field: "presets first, hex
// later" (operator, 2026-09-26). The locked colours (transmit, the critical orange, the need set)
// are not roles, so nothing here can reach them.
//
// The picks and the setter arrive as props from App's usePaletteRoles, the one writer of the
// attributes; this component never touches <html> or storage itself.
//
// ON A BUILT-IN THEME (features/skins.ts) the default of a role the theme retunes — the Accent and
// the Readout — is the theme's own colour, not the standard one, so that chip says "Theme's own"
// and shows the theme's colour. Otherwise the row would claim the standard cyan is what the
// operator sees. A pick still wins over the theme, as it does over the standard colour.
import type { CSSProperties } from 'react'
import { t } from '../i18n'
import { PALETTE_ROLES, defaultPresetId, type PaletteRoleId, type PaletteSelection } from '../features/paletteRoles'
import { skinOf, type SkinId } from '../features/skins'

interface Props {
  palette: PaletteSelection
  /** The built-in theme on screen, or null for a standard one. */
  skin?: SkinId | null
  onChange: (role: PaletteRoleId, presetId: string) => void
}

export function SettingsColours({ palette, skin = null, onChange }: Props) {
  const theme = skinOf(skin)
  return (
    <div className="settings-grid">
      {PALETTE_ROLES.map((role) => {
        const name = t(role.labelKey)
        const current = palette[role.id]
        const standard = defaultPresetId(role)
        return (
          <div className="settings-field" key={role.id}>
            {/* The Reset sits on the name's line, so the chips always get the whole width. */}
            <span className="palette-head">
              <span className="settings-label">{name}</span>
              <button
                type="button"
                className="palette-reset"
                disabled={current === standard}
                aria-label={t('settings.colours.reset.aria', { role: name })}
                title={t('settings.colours.reset.title')}
                onClick={() => onChange(role.id, standard)}
              >
                {t('settings.colours.reset')}
              </button>
            </span>
            <span className="theme-switcher" role="group" aria-label={name}>
              {role.presets.map((p) => {
                const on = p.id === current
                // The theme's own colour, when this is the default of a role the theme retunes.
                const own = p.id === standard ? theme?.day[role.swatch] : undefined
                // Both themes' colours ride on the swatch and the sheet shows the one for the
                // theme in use, so the chip never needs to know which theme that is.
                const swatch = (
                  own ? { '--swatch-dark': own, '--swatch-light': own } : { '--swatch-dark': p.dark[role.swatch], '--swatch-light': p.light[role.swatch] }
                ) as CSSProperties
                return (
                  <button
                    key={p.id}
                    type="button"
                    className={`theme-chip${on ? ' active' : ''}`}
                    aria-pressed={on}
                    onClick={() => onChange(role.id, p.id)}
                  >
                    <span className="palette-swatch" style={swatch} aria-hidden="true" />
                    {own ? t('palette.preset.themeOwn') : t(p.labelKey)}
                  </button>
                )
              })}
            </span>
            <span className="settings-hint">{t(role.hintKey)}</span>
          </div>
        )
      })}
    </div>
  )
}
