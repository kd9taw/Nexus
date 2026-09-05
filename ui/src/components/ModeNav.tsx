import type { OpMode, Tier } from '../types'
import type { LucideIcon } from 'lucide-react'
import {
  Radio,
  Mic,
  Radar,
  Plane,
  Satellite,
  Target,
  Rss,
  MessageSquare,
  Tent,
  Trees,
  BookOpen,
  Trophy,
  BarChart3,
  Zap,
  Cable,
  Bookmark,
  Settings,
  Type,
  Keyboard,
  Image as ImageIcon,
  MapPin,
  RotateCcw,
  Users,
} from 'lucide-react'
import { Fragment, useState, type ButtonHTMLAttributes } from 'react'
import { Tooltip, TooltipProvider } from './ui/Tooltip'
import { t, type MessageKey } from '../i18n'
import { type FeatureId, type View } from '../features/registry'
import { orderNav, moveNav, loadNavOrder, saveNavOrder, resetNavOrder } from '../navOrder'

// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every button's
// TOOLTIP is prose and lives in the catalog. Its LABEL is prose only where the section is
// named for itself: the six mode buttons (FT, Tempo, Phone, CW, RTTY, PSK, SSTV, APRS) and
// the two named for an event or a programme (Field Day, POTA / SOTA) carry their names
// verbatim below, because a mode name and a programme name are the same letters in every
// language (`i18n/index.ts`, the invariant-token rule).

// `View` now lives in the feature registry (features ARE the views); re-export so
// existing `import { type View } from './ModeNav'` call-sites keep working.
export type { View }

interface Props {
  /** Current view selected in the UI. */
  view: View
  /** The operating mode reported by the snapshot (drives the live badge). */
  mode: OpMode
  /** Enabled-set from the feature system — disabled sections are hidden. */
  enabled: Record<FeatureId, boolean>
  onSelect: (view: View) => void
  /** Live radio tier (TempoFast/TempoDeep/FT8/FT4) — picks which Digital sub-item is active. */
  tier: Tier
  /** Choose a Digital sub-mode: 'digital' opens the weak-signal cockpit on its
   * last FT8/FT4 tier; 'tempo' opens the TempoFast/TempoDeep free-text calling cockpit;
   * 'rtty' / 'sstv' open their sections. */
  onDigitalMode: (m: DigitalMode) => void
  /** Open the club band board in its own window (the `fdclub` pop-out). It is a WINDOW,
   * not a `View`, so it takes its own callback rather than an `ITEMS` entry — see the
   * button below the Field Day item. */
  onClubBoard: () => void
}

/** The cockpits grouped under "Digital" in the rail (FT · Tempo · RTTY · PSK · SSTV · APRS). */
export type DigitalMode = 'digital' | 'tempo' | 'rtty' | 'psk' | 'sstv' | 'aprs'

interface DigitalSub {
  mode: DigitalMode
  /** The mode's own name — an invariant token, carried verbatim. */
  label: string
  icon: LucideIcon
  /** Tooltip + accessible name: prose, so a catalog key. */
  titleKey: MessageKey
  /** Whether this sub-item is the active one, given the current view + tier. */
  active: (view: View, tier: Tier) => boolean
}

// One "Digital" button for the weak-signal cockpit (the FT8/FT4 pick lives in
// the top bar's tier pills — Fast · Robust · FT4 · FT8 — separate FT8/FT4 rail
// icons were redundant, operator request) and Tempo for the TempoFast/TempoDeep free-text
// cockpit. The active highlight is view-first so a global view (e.g. Map)
// leaves none of them lit.
const DIGITAL_SUBS: DigitalSub[] = [
  {
    mode: 'digital',
    label: 'FT',
    icon: Radio,
    titleKey: 'nav.digital.ft.title',
    active: (v) => v === 'operate',
  },
  {
    mode: 'tempo',
    label: 'Tempo',
    icon: MessageSquare,
    titleKey: 'nav.digital.tempo.title',
    active: (v) => v === 'chat',
  },
  // RTTY + SSTV are opt-in sections (feature-gated like Phone/CW, on by default) —
  // the render filters them out of the group when disabled.
  {
    mode: 'rtty',
    label: 'RTTY',
    icon: Type,
    titleKey: 'nav.digital.rtty.title',
    active: (v) => v === 'rtty',
  },
  {
    mode: 'psk',
    label: 'PSK',
    icon: Keyboard,
    titleKey: 'nav.digital.psk.title',
    active: (v) => v === 'psk',
  },
  {
    mode: 'sstv',
    label: 'SSTV',
    icon: ImageIcon,
    titleKey: 'nav.digital.sstv.title',
    active: (v) => v === 'sstv',
  },
  {
    mode: 'aprs',
    label: 'APRS',
    icon: MapPin,
    titleKey: 'nav.digital.aprs.title',
    active: (v) => v === 'aprs',
  },
]

interface Item {
  id: View
  label: string
  icon: LucideIcon
  title: string
}

/**
 * One rail button, with its words looked up when they are READ rather than at import.
 *
 * These are module constants a dozen renders index directly, so resolving at import time
 * would freeze whichever locale loaded this module first and no re-render could move it —
 * the `features/needVisuals.ts` lesson. A getter keeps the record's SHAPE (`.label`,
 * `.title`) and does the lookup at the moment the string is read.
 */
function item(v: { id: View; icon: LucideIcon; labelKey: MessageKey; titleKey: MessageKey }): Item {
  return {
    id: v.id,
    icon: v.icon,
    get label() {
      return t(v.labelKey)
    },
    get title() {
      return t(v.titleKey)
    },
  }
}

// The two non-digital operating cockpits, first in the rail (operator order:
// Phone · CW · Digital group). Both opt-in (gated by `enabled`). Their labels are the
// MODES' own names, so they are carried here rather than in the catalog.
const PHONE: Item = {
  id: 'phone',
  label: 'Phone',
  icon: Mic,
  get title() {
    return t('nav.phone.title')
  },
}
const CW: Item = {
  id: 'cw',
  label: 'CW',
  icon: Zap,
  get title() {
    return t('nav.cw.title')
  },
}

// Everything below the operating group: global situational/logging surfaces + opt-in
// extras (all `core: false`, so they appear only when enabled in Settings ▸ Features).
// `operate` and `chat` are NOT here — they live in the Digital group above as FT8/FT4
// and Tempo. ('qso' stays retired from the nav; the Digital cockpit sequences inline.)
// 'band' (Broadcasts) and 'log' (Field Log) have been removed — deleted sections.
export const ITEMS: Item[] = [
  item({ id: 'connect', icon: Radar, labelKey: 'nav.connect.label', titleKey: 'nav.connect.title' }),
  item({ id: 'needed', icon: Target, labelKey: 'nav.needed.label', titleKey: 'nav.needed.title' }),
  item({ id: 'spots', icon: Rss, labelKey: 'nav.spots.label', titleKey: 'nav.spots.title' }),
  item({ id: 'dxped', icon: Plane, labelKey: 'nav.dxped.label', titleKey: 'nav.dxped.title' }),
  item({ id: 'sats', icon: Satellite, labelKey: 'nav.sats.label', titleKey: 'nav.sats.title' }),
  item({ id: 'logbook', icon: BookOpen, labelKey: 'nav.logbook.label', titleKey: 'nav.logbook.title' }),
  item({ id: 'awards', icon: Trophy, labelKey: 'nav.awards.label', titleKey: 'nav.awards.title' }),
  item({ id: 'stats', icon: BarChart3, labelKey: 'nav.stats.label', titleKey: 'nav.stats.title' }),
  // The ARRL event's own name, and the two programmes' — tokens, not prose.
  {
    id: 'fieldDay',
    label: 'Field Day',
    icon: Tent,
    get title() {
      return t('nav.fieldDay.title')
    },
  },
  {
    id: 'pota',
    label: 'POTA/SOTA',
    icon: Trees,
    get title() {
      return t('nav.pota.title')
    },
  },
  item({ id: 'memories', icon: Bookmark, labelKey: 'nav.memories.label', titleKey: 'nav.memories.title' }),
  item({ id: 'program', icon: Cable, labelKey: 'nav.program.label', titleKey: 'nav.program.title' }),
]

// Roam is no longer a rail section — it lives INSIDE the Tempo cockpit
// (header chip + settings panel), per operator request.

// The operating-mode badge. `QSO` is a Q-code and `FIELD DAY` the event's own name — both
// invariant — so only the conversational mode's word resolves from the catalog.
const MODE_LABEL: Record<OpMode, string> = {
  get chat() {
    return t('nav.mode.chat')
  },
  qso: 'QSO',
  fieldDay: 'FIELD DAY',
}

export function ModeNav({ view, mode, enabled, onSelect, tier, onDigitalMode, onClubBoard }: Props) {
  // Operator's drag-and-drop rail order for the global sections (shared across windows).
  // `order` is the persisted id list; `orderNav` folds it over the shipped ITEMS so a new
  // section is never lost and a deleted one is dropped.
  const [order, setOrder] = useState<string[]>(loadNavOrder)
  const [dragId, setDragId] = useState<string | null>(null)
  const [overId, setOverId] = useState<string | null>(null)

  // Sections show purely by feature-enable now (no workspace/area gating) — the old
  // dx/msg split is gone; FT8/FT4/Tempo live in the Digital group instead. Reorder by the
  // operator's saved order first, THEN drop disabled ones (so the saved order is stable
  // regardless of which sections are currently enabled).
  const orderedIds = orderNav(
    ITEMS.map((it) => it.id),
    order,
  )
  const items = orderedIds
    .map((id) => ITEMS.find((it) => it.id === id)!)
    .filter((it) => enabled[it.id] !== false)
  const customized = order.length > 0

  const dropOn = (targetId: string | null) => {
    if (!dragId) return
    const next = moveNav(orderedIds, dragId, targetId)
    setOrder(next)
    saveNavOrder(next)
    setDragId(null)
    setOverId(null)
  }
  const resetOrder = () => {
    resetNavOrder()
    setOrder([])
  }
  // A plain view button (used for Phone, CW, and the global sections). `dragProps` are spread
  // onto the button itself — the drag SOURCE must be the button, not a wrapping div: a form
  // control (button) inside a `draggable` ancestor swallows the press gesture, so the ancestor
  // drag never starts. Placed before the fixed props so `onClick`/`className` can't be clobbered.
  const navBtn = (it: Item, dragProps?: ButtonHTMLAttributes<HTMLButtonElement>) => {
    const Icon = it.icon
    return (
      <Tooltip key={it.id} content={it.title}>
        <button
          type="button"
          {...dragProps}
          className={`mode-btn${view === it.id ? ' active' : ''}`}
          aria-current={view === it.id ? 'page' : undefined}
          aria-label={it.title}
          onClick={() => onSelect(it.id)}
        >
          <span className="mode-glyph" aria-hidden="true">
            <Icon size={18} strokeWidth={1.75} />
          </span>
          <span className="mode-label">{it.label}</span>
        </button>
      </Tooltip>
    )
  }
  return (
    <TooltipProvider>
      <nav className="mode-nav" aria-label={t('nav.aria')}>
        <div className="mode-nav-top">
          {/* Operating group order (operator spec): Phone · CW · Digital group
              (FT + Tempo). The FT8/FT4 pick lives in the top bar's tier pills. */}
          {enabled.phone !== false && navBtn(PHONE)}
          {enabled.cw !== false && navBtn(CW)}
          <div className="mode-nav-group" role="group" aria-label={t('nav.digital.group.aria')}>
            <span className="mode-nav-group-label">{t('nav.digital.group.label')}</span>
            {DIGITAL_SUBS.filter(
              // FT + Tempo are core (always shown); RTTY/SSTV hide when disabled
              // in Settings ▸ Features (their DigitalMode doubles as FeatureId).
              (s) => s.mode === 'digital' || s.mode === 'tempo' || enabled[s.mode] !== false,
            ).map((s) => {
              const Icon = s.icon
              const active = s.active(view, tier)
              const title = t(s.titleKey)
              return (
                <Tooltip key={s.mode} content={title}>
                  <button
                    type="button"
                    className={`mode-btn sub${active ? ' active' : ''}`}
                    aria-current={active ? 'page' : undefined}
                    aria-label={title}
                    onClick={() => onDigitalMode(s.mode)}
                  >
                    <span className="mode-glyph" aria-hidden="true">
                      <Icon size={16} strokeWidth={1.75} />
                    </span>
                    <span className="mode-label">{s.label}</span>
                  </button>
                </Tooltip>
              )
            })}
          </div>
          {/* Global situational/logging surfaces + opt-in extras — drag to reorder. */}
          {items.map((it) => (
            <Fragment key={it.id}>
              <div
                className={`mode-nav-drag${dragId === it.id ? ' dragging' : ''}${
                  overId === it.id ? ' dragover' : ''
                }`}
              >
                {navBtn(it, {
                  draggable: true,
                  onDragStart: (e) => {
                    setDragId(it.id)
                    e.dataTransfer.effectAllowed = 'move'
                    // Required by some engines (and harmless elsewhere) for the drag to begin.
                    e.dataTransfer.setData('text/plain', it.id)
                  },
                  onDragOver: (e) => {
                    if (dragId && dragId !== it.id) {
                      e.preventDefault()
                      e.dataTransfer.dropEffect = 'move'
                      setOverId(it.id)
                    }
                  },
                  onDragLeave: () => setOverId((o) => (o === it.id ? null : o)),
                  onDrop: (e) => {
                    e.preventDefault()
                    dropOn(it.id)
                  },
                  onDragEnd: () => {
                    setDragId(null)
                    setOverId(null)
                  },
                })}
              </div>
              {/* The club band board, straight to its own window. It rides WITH the Field Day
                  item (including through a drag-reorder) because that is the only place an
                  operator looks for it, and it is gated on the SAME switch — the FD master
                  switch, never on club sync. Gating it on sync is what hid it: the board only
                  existed inside FieldDayView once `fieldDay.club` was non-null, so an operator
                  who had not already turned sync on had no way to learn it was there. */}
              {it.id === 'fieldDay' && (
                <Tooltip content={t('nav.fdClub.title')}>
                  <button
                    type="button"
                    className="mode-btn"
                    aria-label={t('nav.fdClub.title')}
                    onClick={onClubBoard}
                  >
                    <span className="mode-glyph" aria-hidden="true">
                      <Users size={18} strokeWidth={1.75} />
                    </span>
                    <span className="mode-label">{t('nav.fdClub.label')}</span>
                  </button>
                </Tooltip>
              )}
            </Fragment>
          ))}
          {customized && (
            <button
              type="button"
              className="mode-nav-reset"
              title={t('nav.order.reset.title')}
              onClick={resetOrder}
            >
              <RotateCcw size={13} strokeWidth={1.75} aria-hidden="true" />
              <span className="mode-label">{t('nav.order.reset.label')}</span>
            </button>
          )}
        </div>

        <div className="mode-nav-bottom">
          <span className="mode-current" title={t('nav.mode.title')}>
            <span className="mode-current-dot" aria-hidden="true" />
            {MODE_LABEL[mode]}
          </span>
          {/* One word for the gear: tooltip, accessible name and visible label are the same
              claim about the same button, so they are one entry. */}
          <Tooltip content={t('nav.settings.label')}>
            <button
              type="button"
              className={`mode-btn gear${view === 'settings' ? ' active' : ''}`}
              aria-current={view === 'settings' ? 'page' : undefined}
              aria-label={t('nav.settings.label')}
              onClick={() => onSelect('settings')}
            >
              <span className="mode-glyph" aria-hidden="true">
                <Settings size={18} strokeWidth={1.75} />
              </span>
              <span className="mode-label">{t('nav.settings.label')}</span>
            </button>
          </Tooltip>
        </div>
      </nav>
    </TooltipProvider>
  )
}
