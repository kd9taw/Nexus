// One grid slot: a header (pane title, a content-picker to reassign the slot, and ✕ to close
// it) over a body that renders the pane's full panel, falling back to its one-sentence
// projection when there is no data yet. The picker auto-lists every registry entry under its group,
// so a new pane appears with no change here.
//
// The Basic/Expert detail toggle was removed 2026-07-26 (operator) — every pane renders in full.
// `def.basic()` is NOT the removed mode: it is the loading / no-data / offline hint for every
// pane, reached whenever `def.expert()` returns null. Deleting it would blank a pane that is
// simply waiting on a feed.
//
// CLOSE + RESIZE (2026-09-13). ✕ closes the SLOT (panelState 'removed'); ⊞ Panels in the
// Connect header brings it back with the same pane in it. No `className`/`style` prop — the
// frame cannot size itself (the pane-grid contract). A rail frame's split is a typed `share`
// placed inline, exactly like CockpitPaneFrame's `weight`: `--connect-share` is the variable
// the rail separator paints during a drag, and `--connect-pane-flex` is the xs stack's
// content-height override. A strip frame takes no share; its grid sizes it.
//
// THE BOX'S OWN TEXT SIZE (2026-09-29, ⋯ ▸ A− / A+). A typed `textScale` placed inline on the
// BODY as `--box-text-scale`, the same kind of placement input as the share: styles.css multiplies
// the app's --text-scale by it for everything inside the body, and the head — the title, the picker,
// the ⋯ and ✕ — stays at the app's size, so a box's head is the same height at every size and the
// strip's row does not jump. The factor is written only when it is not 1: a box at the app's size
// renders exactly the DOM it rendered before the control existed. Every prop here is optional, so a
// host that passes none of them gets today's frame — plus the ⋯ menu's manual link, which needs no
// host at all (connect/paneHelp): the ⋯ renders whenever the menu has something in it.
//
// TABS (2026-09-29): a slot may hold several panes (features/connectConfig `tabs`). With two or more,
// the title becomes a TAB STRIP — the WAI-ARIA tabs pattern: one button per pane in the title's own
// face, the shown one selected and the only one in the Tab order, ←/→ (and Home/End) moving to the
// next pane and showing it, the body the tabpanel. With one pane the head is the title, as it always
// was. The picker replaces the SHOWN pane; ⋯ ▸ Add a tab and ⋯ ▸ Remove are the menu's. A tabbed head
// is marked `data-tabs`: styles.css lets it wrap, its controls on top, when they and the strip do not
// fit side by side.
//
// AUTO-ROTATE (2026-09-29, the dashboard window and the TV page only — the host passes `rotateSecs`
// and `onRotate` only there): with an interval set, the frame shows its next tab every interval, round
// the slot. It PAUSES while the pointer is over the frame, while anything in it has the KEYBOARD focus
// and while its ⋯ menu is open, and a pause ends with a fresh interval. Keyboard focus, judged as
// :focus-visible judges it: the focus a mouse click leaves on the tab or the ⋯ it pressed does not
// hold the slot once the pointer has gone, or a slot clicked once would never rotate again. The count restarts only when
// the interval, the shown tab or the slot's tabs change — never on an ordinary re-render, which the
// dashboard window does on every snapshot. Off (no interval) by default.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). Every pane's name
// arrives already translated from the registry (`panes.tsx`, resolved through getters), and so do
// the picker's groups, named by what the boxes are for (`PANE_CATEGORY_LABEL`). The ✕ uses the
// cockpit frame's own words (`pane.hide.*`) — one gesture, one sentence, in every view.
//
// THE DASHBOARD RAIL beside the cockpits renders these same frames in its own four slots
// (components/DashRail): the slot id is the host's, and `frameRef` hands a divider between two
// frames (PaneSeam) the boxes it measures and repaints — a ref, never a size.
import { useEffect, useId, useRef, useState, type CSSProperties, type KeyboardEvent, type Ref } from 'react'
import { t } from '../../i18n'
import { PANES, PANE_CATEGORIES, PANE_CATEGORY_LABEL, paneById } from './panes'
import { BoxMenu } from './BoxMenu'
import { paneHelpUrl } from './paneHelp'
import type { PaneContext } from './paneContext'
import type { PaneId, SlotId } from '../../features/connectConfig'

/** Was the last press a pointer's (true) or a key's (false)? Document-level and capturing, so a Tab
 *  pressed outside the slot counts; installed once, by the first frame. */
let lastPressWasPointer = false
let watchingPresses = false
function watchPresses() {
  if (watchingPresses || typeof document === 'undefined') return
  watchingPresses = true
  document.addEventListener('pointerdown', () => (lastPressWasPointer = true), true)
  document.addEventListener('keydown', () => (lastPressWasPointer = false), true)
}

export function PaneFrame<S extends string = SlotId>({
  slotId,
  slotName,
  paneId,
  ctx,
  onAssign,
  share,
  onHide,
  frameRef,
  textScale,
  onTextScale,
  tabs,
  onShowTab,
  addable,
  onAddTab,
  onRemoveTab,
  rotateSecs,
  onRotate,
  marked,
}: {
  slotId: S
  /** The slot as the picker's accessible name says it. Omitted ⇒ the slot id (Connect's). */
  slotName?: string
  /** The pane SHOWN in this slot. */
  paneId: PaneId
  ctx: PaneContext
  onAssign: (slotId: S, paneId: PaneId) => void
  /** Rail frames only: this pane's flex share of its rail (default split 1:1). */
  share?: number
  /** Close this slot. Omitted ⇒ no ✕. */
  onHide?: () => void
  /** The frame's own box, for a divider beside it to measure and repaint. Omitted ⇒ no ref. */
  frameRef?: Ref<HTMLElement>
  /** This box's text size, a factor on the app's Text size (⋯ ▸ A− / A+). Omitted ⇒ 1. */
  textScale?: number
  /** Change it. Omitted ⇒ the ⋯ menu offers no text size. */
  onTextScale?: (factor: number) => void
  /** Every pane in this slot, in tab order (connectConfig `slotBoxes`). Omitted or one ⇒ no tabs. */
  tabs?: readonly PaneId[]
  /** Show one of them. */
  onShowTab?: (paneId: PaneId) => void
  /** What ⋯ ▸ Add a tab offers (connectConfig `addableTo`). */
  addable?: readonly PaneId[]
  /** Add one as a tab. Omitted ⇒ no Add a tab. */
  onAddTab?: (paneId: PaneId) => void
  /** Take the shown pane out of the slot — offered only while the slot holds two or more. */
  onRemoveTab?: () => void
  /** The panes on screen elsewhere, which the picker marks: beside a cockpit, what the cockpit and the rail's
   *  other slots show (once per screen across the two: choosing one moves it here). Omitted ⇒ none marked. */
  marked?: ReadonlySet<PaneId>
  /** Seconds between tabs, when this slot rotates (the dashboard window and the TV page). */
  rotateSecs?: number
  /** Set or clear it. Omitted ⇒ the menu offers no rotation (the main window). */
  onRotate?: (secs: number | null) => void
}) {
  const uid = useId()
  // Auto-rotate. Hooks first: the frame returns null below for an unknown pane.
  const [pointerIn, setPointerIn] = useState(false)
  const [focusIn, setFocusIn] = useState(false)
  const [menuOpen, setMenuOpen] = useState(false)
  const showTabNow = useRef(onShowTab)
  useEffect(() => {
    showTabNow.current = onShowTab
  })
  useEffect(watchPresses, [])
  const tabsKey = tabs && tabs.length > 1 ? tabs.join(' ') : ''
  const paused = pointerIn || focusIn || menuOpen
  useEffect(() => {
    if (!rotateSecs || !tabsKey || paused) return
    const order = tabsKey.split(' ') as PaneId[]
    const id = window.setTimeout(() => {
      showTabNow.current?.(order[(order.indexOf(paneId) + 1) % order.length])
    }, rotateSecs * 1000)
    return () => window.clearTimeout(id)
  }, [rotateSecs, tabsKey, paneId, paused])
  const def = paneById(paneId)
  if (!def) return null
  const scale = textScale ?? 1
  const helpUrl = paneHelpUrl(paneId)
  const tabbed = tabs && tabs.length > 1 ? tabs : null
  const tabId = (p: PaneId) => `${uid}-tab-${p}`
  const panelId = `${uid}-panel`
  // ←/→ wrap, Home/End: the WAI-ARIA tabs keys, showing the tab they reach (automatic activation).
  const onTabKey = (e: KeyboardEvent<HTMLButtonElement>) => {
    if (!tabbed) return
    const i = tabbed.indexOf(paneId)
    const n = tabbed.length
    const to =
      e.key === 'ArrowRight' ? (i + 1) % n : e.key === 'ArrowLeft' ? (i - 1 + n) % n : e.key === 'Home' ? 0 : e.key === 'End' ? n - 1 : -1
    if (to < 0) return
    e.preventDefault()
    onShowTab?.(tabbed[to])
    ;(e.currentTarget.parentElement?.children[to] as HTMLElement | undefined)?.focus({ preventScroll: true })
  }
  return (
    <section
      ref={frameRef}
      className="pane-frame"
      data-slot={slotId}
      data-pane={paneId}
      onPointerEnter={() => setPointerIn(true)}
      onPointerLeave={() => setPointerIn(false)}
      onFocus={() => {
        if (!lastPressWasPointer) setFocusIn(true)
      }}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget as Node | null)) setFocusIn(false)
      }}
      style={
        share === undefined
          ? undefined
          : ({ '--connect-share': share, flex: 'var(--connect-pane-flex, var(--connect-share) 1 0)' } as CSSProperties)
      }
    >
      <header className="pane-head" data-tabs={tabbed ? '' : undefined}>
        {tabbed ? (
          <div className="pane-tabs" role="tablist" aria-label={t('connect.box.tabs.aria')}>
            {tabbed.map((p) => {
              const on = p === paneId
              return (
                <button
                  key={p}
                  type="button"
                  role="tab"
                  id={tabId(p)}
                  className={`pane-tab${on ? ' active' : ''}`}
                  aria-selected={on}
                  aria-controls={panelId}
                  tabIndex={on ? 0 : -1}
                  onClick={() => onShowTab?.(p)}
                  onKeyDown={onTabKey}
                >
                  {paneById(p)?.title ?? p}
                </button>
              )
            })}
          </div>
        ) : (
          <span className="pane-title">{def.title}</span>
        )}
        <div className="pane-acts">
          <select
            className="pane-pick"
            value={paneId}
            aria-label={t('connect.slot.pick.aria', { slot: slotName ?? slotId })}
            title={t('connect.slot.pick.title')}
            onChange={(e) => onAssign(slotId, e.target.value as PaneId)}
          >
            {PANE_CATEGORIES.map((cat) => {
              const items = PANES.filter((p) => p.category === cat)
              return items.length ? (
                <optgroup key={cat} label={PANE_CATEGORY_LABEL[cat]()}>
                  {items.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.id !== paneId && marked?.has(p.id) ? t('panels.box.pick.onScreen', { title: p.title }) : p.title}
                    </option>
                  ))}
                </optgroup>
              ) : null
            })}
          </select>
          {(onTextScale || helpUrl || onAddTab) && (
            <BoxMenu
              title={def.title}
              textScale={scale}
              onTextScale={onTextScale}
              helpUrl={helpUrl}
              addable={addable}
              onAddTab={onAddTab}
              onRemoveTab={tabbed ? onRemoveTab : undefined}
              rotateSecs={rotateSecs}
              onRotate={tabbed ? onRotate : undefined}
              onOpenChange={setMenuOpen}
            />
          )}
          {onHide && (
            <button
              type="button"
              className="pane-close"
              onClick={onHide}
              aria-label={t('pane.hide.aria', { title: def.title })}
              title={t('pane.hide.title')}
            >
              ✕
            </button>
          )}
        </div>
      </header>
      <div
        className="pane-body"
        {...(tabbed ? { role: 'tabpanel', id: panelId, 'aria-labelledby': tabId(paneId) } : {})}
        style={scale === 1 ? undefined : ({ '--box-text-scale': scale } as CSSProperties)}
      >
        <PaneBody pane={paneId} ctx={ctx} />
      </div>
    </section>
  )
}

/** A box's body: the pane's full panel, or its one-line state while the panel has nothing to show
 *  (`expert` returns null → `basic`). The ONE renderer of a box's body, wherever the box stands — this
 *  frame on Conditions, in the dashboard window and in the rail, and any area that shows an entry of
 *  the shared list (features/sharedPanes) — so no surface can draw a box differently. */
export function PaneBody({ pane, ctx }: { pane: PaneId; ctx: PaneContext }) {
  const def = paneById(pane)
  if (!def) return null
  return <>{def.expert(ctx) ?? <p className="pane-basic">{def.basic(ctx)}</p>}</>
}
