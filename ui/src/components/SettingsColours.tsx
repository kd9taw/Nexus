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
import type { CSSProperties } from 'react'
import { t } from '../i18n'
import { PALETTE_ROLES, defaultPresetId, type PaletteRoleId, type PaletteSelection } from '../features/paletteRoles'

interface Props {
  palette: PaletteSelection
  onChange: (role: PaletteRoleId, presetId: string) => void
}

export function SettingsColours({ palette, onChange }: Props) {
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
                // Both themes' colours ride on the swatch and the sheet shows the one for the
                // theme in use, so the chip never needs to know which theme that is.
                const swatch = { '--swatch-dark': p.dark[role.swatch], '--swatch-light': p.light[role.swatch] } as CSSProperties
                return (
                  <button
                    key={p.id}
                    type="button"
                    className={`theme-chip${on ? ' active' : ''}`}
                    aria-pressed={on}
                    onClick={() => onChange(role.id, p.id)}
                  >
                    <span className="palette-swatch" style={swatch} aria-hidden="true" />
                    {t(p.labelKey)}
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
