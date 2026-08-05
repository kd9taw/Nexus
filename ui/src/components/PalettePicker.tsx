import { MASTER_PALETTES } from '../waterfall'
import { useWaterfallPalette } from '../waterfallPalette'

/**
 * The waterfall-palette picker. Dropped into each cockpit's scope header.
 *
 * Unscoped (the default) it drives the MASTER value shared by the CW and Phone scopes and
 * the RTTY/SSTV waterfalls — every instance shows the same value, so changing it in any of
 * those modes updates them all live. Given a `scope` it drives that scope's own key
 * instead, and only surfaces on the same scope follow it (the FT waterfall — see
 * `waterfallPalette.ts`). `'auto'` rides the active theme.
 *
 * The label has to tell the truth about which of those two a given picker is, or the
 * operator learns the control lies.
 */
export function PalettePicker({
  className = 'wf-palette',
  scope,
}: {
  className?: string
  scope?: string
}) {
  const [palette, setPalette] = useWaterfallPalette(scope)
  const label = scope
    ? 'Waterfall color palette (this mode)'
    : 'Waterfall color palette (applies to all modes)'
  return (
    <select
      className={className}
      value={palette}
      aria-label={label}
      title={
        scope
          ? 'Waterfall color palette — applies to this mode'
          : 'Waterfall color palette — applies to every mode'
      }
      onChange={(e) => setPalette(e.target.value)}
    >
      {MASTER_PALETTES.map((p) => (
        <option key={p.value} value={p.value}>
          {p.label}
        </option>
      ))}
    </select>
  )
}
