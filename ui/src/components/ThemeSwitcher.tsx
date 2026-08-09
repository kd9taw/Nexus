import type { Theme } from '../useTheme'

interface Props {
  theme: Theme
  onChange: (t: Theme) => void
  /** FIELD MODE (outdoor/POTA): bigger type + high contrast, one tap, obviously reversible.
   *  Optional so existing render sites without the wiring keep exactly their old chips. */
  field?: boolean
  onFieldChange?: (on: boolean) => void
}

const OPTIONS: { id: Theme; label: string; title: string }[] = [
  { id: 'light', label: 'Light', title: 'Light (sunlight)' },
  { id: 'dark', label: 'Dark', title: 'Dark (shack)' },
]

export function ThemeSwitcher({ theme, onChange, field, onFieldChange }: Props) {
  return (
    <div className="theme-switcher" role="group" aria-label="Theme">
      {OPTIONS.map((o) => (
        <button
          key={o.id}
          type="button"
          title={o.title}
          aria-pressed={theme === o.id}
          className={`theme-chip${theme === o.id ? ' active' : ''}`}
          onClick={() => onChange(o.id)}
        >
          {o.label}
        </button>
      ))}
      {onFieldChange && (
        <button
          type="button"
          // The tooltip names light as the better outdoor base WITHOUT flipping the
          // operator's theme for them — the physics favours light outdoors (an emissive
          // dark background is dominated by reflection), but a silent theme change is an
          // invisible decision. One string instead.
          title={
            field
              ? 'Field mode is on: larger type, maximum contrast. Click to turn off.'
              : 'Field mode for operating outdoors: larger type, maximum contrast (Light theme reads best in daylight)'
          }
          aria-pressed={field === true}
          className={`theme-chip field-chip${field ? ' active' : ''}`}
          onClick={() => onFieldChange(!field)}
        >
          Field
        </button>
      )}
    </div>
  )
}
