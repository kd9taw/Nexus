// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The chip and badge
// WORDS come from the catalog; the CSS class, the icon and the precedence order are code.
// POTA and SOTA are the programmes' own names — invariant tokens, kept below as constants
// (the rule is in `i18n/index.ts`).
//
// THE WORDS RESOLVE LAZILY, through getters. These two records are module constants that a
// dozen surfaces read during render, so resolving them at import time would freeze whichever
// locale was active when this module first loaded and no re-render could ever move it. A
// getter keeps the record's SHAPE — every consumer still reads `.label` / `.short` / `.title`
// — and does the lookup at the moment the string is read.
import {
  Globe2,
  Compass,
  Layers,
  Radio,
  Grid3x3,
  MapPin,
  Tent,
  MailQuestion,
  TreePine,
  Mountain,
  Star,
  type LucideIcon,
} from 'lucide-react'
import { t, type MessageKey } from '../i18n'

/** The programmes' own names — the same four letters in every language. */
const POTA_PROGRAM = 'POTA'
const SOTA_PROGRAM = 'SOTA'

/** A reason a heard station is worth working, in one vocabulary shared by the Needed
 * panel and the band-activity decode feed so the two views read as one system. */
export type NeedCat =
  | 'entity'
  | 'zone'
  | 'band'
  | 'mode'
  | 'grid'
  | 'state'
  | 'dxped'
  | 'confirm'
  | 'pota'
  | 'sota'
  | 'wanted'

export interface NeedVisual {
  /** CSS class suffix — pairs with the `--need-*` palette (`.decode-row.need-*`,
   * `.need-chip.need-*`, `.np-row.need-*`). */
  cls: string
  Icon: LucideIcon
  /** Short text badge — the SAME vocabulary the Needed panel's chips use, so the decode
   * feed and the board read identically ("NEW ONE", "BAND", "POTA"…). */
  label: string
  title: string
  /** Icon-only categories that must NOT drive row colour (mirrors NeededPanel, where
   * dxped/pota/sota are appended and never `tags[0]`); the award tier keeps the colour. */
  iconOnly?: boolean
}

/** One badge, with its words looked up when they are read rather than at import. */
function badge(v: {
  cls: string
  Icon: LucideIcon
  labelKey: MessageKey
  titleKey: MessageKey
  iconOnly?: boolean
}): NeedVisual {
  return {
    cls: v.cls,
    Icon: v.Icon,
    iconOnly: v.iconOnly,
    get label() {
      return t(v.labelKey)
    },
    get title() {
      return t(v.titleKey)
    },
  }
}

export const NEED_VISUALS: Record<NeedCat, NeedVisual> = {
  entity: badge({ cls: 'need-entity', Icon: Globe2, labelKey: 'need.badge.entity.label', titleKey: 'need.badge.entity.title' }),
  zone: badge({ cls: 'need-zone', Icon: Compass, labelKey: 'need.badge.zone.label', titleKey: 'need.badge.zone.title' }),
  band: badge({ cls: 'need-band', Icon: Layers, labelKey: 'need.badge.band.label', titleKey: 'need.badge.band.title' }),
  mode: badge({ cls: 'need-mode', Icon: Radio, labelKey: 'need.badge.mode.label', titleKey: 'need.badge.mode.title' }),
  grid: badge({ cls: 'need-grid', Icon: Grid3x3, labelKey: 'need.badge.grid.label', titleKey: 'need.badge.grid.title' }),
  state: badge({ cls: 'need-state', Icon: MapPin, labelKey: 'need.badge.state.label', titleKey: 'need.badge.state.title' }),
  dxped: badge({ cls: 'need-dxped', Icon: Tent, labelKey: 'need.badge.dxped.label', titleKey: 'need.badge.dxped.title', iconOnly: true }),
  confirm: badge({ cls: 'need-confirm', Icon: MailQuestion, labelKey: 'need.badge.confirm.label', titleKey: 'need.badge.confirm.title' }),
  // The two programme names are tokens, so these entries carry the label directly.
  pota: {
    cls: 'need-pota',
    Icon: TreePine,
    label: POTA_PROGRAM,
    get title() {
      return t('need.badge.pota.title')
    },
    iconOnly: true,
  },
  sota: {
    cls: 'need-sota',
    Icon: Mountain,
    label: SOTA_PROGRAM,
    get title() {
      return t('need.badge.sota.title')
    },
    iconOnly: true,
  },
  wanted: badge({ cls: 'need-wanted', Icon: Star, labelKey: 'need.badge.wanted.label', titleKey: 'need.badge.wanted.title' }),
}

/** Canonical precedence (icon order left→right; also picks the row colour): the most
 * chase-worthy reason first. */
export const NEED_PRECEDENCE: NeedCat[] = [
  // MUST mirror the backend NeedTag::tier() descending (needalert.rs) so the decode feed's
  // lead pill + row colour match the Needed board for every multi-need station: the board
  // sorts by tier and colours from tags[0]. tier(): entity 100 > zone 70 > state 60 > grid
  // 55 > band 50 > mode 30 > confirm 10 (wanted pinned top; dxped/pota/sota are icon-only).
  // A rare grid still floats above via the backend rarity_boost; a *plain* grid < state.
  'wanted',
  'entity',
  'zone',
  'state',
  'grid',
  'band',
  'mode',
  'dxped',
  'confirm',
  'pota',
  'sota',
]

/** The need-chip vocabulary — the ONE source for chip text/class/tooltip per
 * NeedTag (this record used to be duplicated with drifting wording across
 * StationCard, OperateRoster, NeededPanel, and the Connect paneFormat).
 * `label` is the full board wording; `short` is for dense columns — the
 * surface picks, the words stay in one place. */
interface NeedChip {
  label: string
  short: string
  cls: string
  title: string
}

/** One chip, with its words looked up when they are read rather than at import. */
function chip(v: {
  cls: string
  labelKey: MessageKey
  shortKey: MessageKey
  titleKey: MessageKey
}): NeedChip {
  return {
    cls: v.cls,
    get label() {
      return t(v.labelKey)
    },
    get short() {
      return t(v.shortKey)
    },
    get title() {
      return t(v.titleKey)
    },
  }
}

export const NEED_CHIP: Record<import('../types').NeedTag, NeedChip> = {
  NewEntity: chip({
    cls: 'entity',
    labelKey: 'need.chip.newEntity.label',
    shortKey: 'need.chip.newEntity.short',
    titleKey: 'need.chip.newEntity.title',
  }),
  NewZone: chip({
    cls: 'zone',
    labelKey: 'need.chip.newZone.label',
    shortKey: 'need.chip.newZone.short',
    titleKey: 'need.chip.newZone.title',
  }),
  NewBand: chip({
    cls: 'band',
    labelKey: 'need.chip.newBand.label',
    shortKey: 'need.chip.newBand.short',
    titleKey: 'need.chip.newBand.title',
  }),
  NewMode: chip({
    cls: 'mode',
    labelKey: 'need.chip.newMode.label',
    shortKey: 'need.chip.newMode.short',
    titleKey: 'need.chip.newMode.title',
  }),
  NewGrid: chip({
    cls: 'grid',
    labelKey: 'need.chip.newGrid.label',
    shortKey: 'need.chip.newGrid.short',
    titleKey: 'need.chip.newGrid.title',
  }),
  NewState: chip({
    cls: 'state',
    labelKey: 'need.chip.newState.label',
    shortKey: 'need.chip.newState.short',
    titleKey: 'need.chip.newState.title',
  }),
  // The subject is the AWARD SLOT (entity/zone/grid/state on this band), never the
  // callsign in the row — CONFIRM-without-B4 is the normal case, not a bug (an operator
  // reasonably read the old "Worked —" as a claim about the station, 2026-08-16).
  Confirm: chip({
    cls: 'confirm',
    labelKey: 'need.chip.confirm.label',
    shortKey: 'need.chip.confirm.short',
    titleKey: 'need.chip.confirm.title',
  }),
  Dxped: chip({
    cls: 'dxped',
    labelKey: 'need.chip.dxped.label',
    shortKey: 'need.chip.dxped.short',
    titleKey: 'need.chip.dxped.title',
  }),
  // The programme names are tokens in both the full and the dense form.
  Pota: {
    label: POTA_PROGRAM,
    short: POTA_PROGRAM,
    cls: 'pota',
    get title() {
      return t('need.chip.pota.title')
    },
  },
  Sota: {
    label: SOTA_PROGRAM,
    short: SOTA_PROGRAM,
    cls: 'sota',
    get title() {
      return t('need.chip.sota.title')
    },
  },
  Wanted: chip({
    cls: 'wanted',
    labelKey: 'need.chip.wanted.label',
    shortKey: 'need.chip.wanted.short',
    titleKey: 'need.chip.wanted.title',
  }),
}
