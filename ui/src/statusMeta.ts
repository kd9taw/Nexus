// Single source of truth pairing each semantic status role with its CSS token,
// its CVD-immune glyph, and a human label. Components MUST render color + glyph
// together (never color alone) — import from here so the pairing can't drift.
// Values are verified in ui/design/verify.mjs; see ui/DESIGN.md.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The CSS token and
// the glyph are code; only the LABEL is read, and it comes from the catalog — through a
// getter, because this is a module constant read during render, and resolving it at import
// time would freeze whichever locale was active when the module first loaded.

import { t, type MessageKey } from './i18n'

export type StatusRole =
  | 'new-entity'
  | 'new-band'
  | 'new-mode'
  | 'worked'
  | 'confirmed'
  | 'dupe'
  | 'snr-strong'
  | 'snr-marginal'
  | 'snr-weak'
  | 'tx'
  | 'rx'
  | 'band-open'
  | 'band-marginal'
  | 'band-closed'
  | 'alert-critical'
  | 'alert-warning'
  | 'alert-info'

export interface StatusMeta {
  /** CSS custom property carrying this role's color (theme-resolved). */
  cssVar: string
  /** Color-independent glyph — the primary identifier (CVD-immune). */
  glyph: string
  label: string
}

/** One role, with its label looked up when it is read rather than at import. */
function meta(v: { cssVar: string; glyph: string; labelKey: MessageKey }): StatusMeta {
  return {
    cssVar: v.cssVar,
    glyph: v.glyph,
    get label() {
      return t(v.labelKey)
    },
  }
}

export const STATUS: Record<StatusRole, StatusMeta> = {
  'new-entity': meta({ cssVar: '--status-new-entity', glyph: '★', labelKey: 'status.newEntity.label' }),
  'new-band': meta({ cssVar: '--status-new-band', glyph: '◑', labelKey: 'status.newBand.label' }),
  'new-mode': meta({ cssVar: '--status-new-mode', glyph: '◧', labelKey: 'status.newMode.label' }),
  worked: meta({ cssVar: '--status-worked', glyph: '○', labelKey: 'status.worked.label' }),
  confirmed: meta({ cssVar: '--status-confirmed', glyph: '✓', labelKey: 'status.confirmed.label' }),
  dupe: meta({ cssVar: '--status-dupe', glyph: '·', labelKey: 'status.dupe.label' }),
  'snr-strong': meta({ cssVar: '--snr-strong', glyph: '▇', labelKey: 'status.snrStrong.label' }),
  'snr-marginal': meta({ cssVar: '--snr-marginal', glyph: '▅', labelKey: 'status.snrMarginal.label' }),
  'snr-weak': meta({ cssVar: '--snr-weak', glyph: '▂', labelKey: 'status.snrWeak.label' }),
  tx: meta({ cssVar: '--tx', glyph: '▲', labelKey: 'status.tx.label' }),
  rx: meta({ cssVar: '--rx', glyph: '▼', labelKey: 'status.rx.label' }),
  'band-open': meta({ cssVar: '--band-open', glyph: '●', labelKey: 'status.bandOpen.label' }),
  'band-marginal': meta({ cssVar: '--band-marginal', glyph: '◐', labelKey: 'status.bandMarginal.label' }),
  'band-closed': meta({ cssVar: '--band-closed', glyph: '⊘', labelKey: 'status.bandClosed.label' }),
  'alert-critical': meta({ cssVar: '--alert-critical', glyph: '⚑', labelKey: 'status.alertCritical.label' }),
  'alert-warning': meta({ cssVar: '--alert-warning', glyph: '△', labelKey: 'status.alertWarning.label' }),
  'alert-info': meta({ cssVar: '--alert-info', glyph: 'i', labelKey: 'status.alertInfo.label' }),
}

/** `var(--token)` for a role's color. */
export function statusColor(role: StatusRole): string {
  return `var(${STATUS[role].cssVar})`
}
