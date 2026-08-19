// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The palette NAMES
// are `MASTER_PALETTES`' own (`waterfall.ts`, which this batch does not own) and arrive as
// data; the `value` of each option is the persisted token and is never prose.
import { t } from '../i18n'
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
  // ⚠️ "every mode" STOPPED BEING TRUE when FT took its own scoped key (4c4fe3da), and this
  // branch was left saying it — the exact failure the doc block above forbids. An operator
  // changing the palette in Phone, CW, RTTY or SSTV was told it reached every mode while it
  // silently did not reach the FT waterfall. Name the modes it actually drives.
  const label = scope
    ? t('waterfall.palette.aria.scoped')
    : t('waterfall.palette.aria.shared')
  return (
    <select
      className={className}
      value={palette}
      aria-label={label}
      title={scope ? t('waterfall.palette.title.scoped') : t('waterfall.palette.title.shared')}
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
