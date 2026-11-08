// THE BEST TIMES CARD — the answer side of Best times (BestTimesAnswer) in a small floating card,
// opened by a call menu's "Best times" item: now, the next three windows, the week, what it rests on,
// and See full day. No chart; that is the Conditions box's.
//
// ⛔ IT NEVER COVERS A TRANSMIT CONTROL. Placed by the station card's own `placeCallCard`, which keeps
// it off every visible TX strip, dock and Stop button (`TX_AVOID`). Where there is no room at all it
// does not show: it goes to See full day instead, so the choice is never a click that does nothing.
// Geometry, so jsdom cannot see it; a real browser can (scripts/browser-probe).
//
// ONE FLOATING CARD AT A TIME: opening this closes the station card, and opening the station card
// closes this (`closeOtherCards`).
//
// SEE FULL DAY opens Conditions in a window of its own (the dashboard window), so the cockpit and its
// Stop TX stay on screen, and hands the call across (`handOff`): that window shows its Best times box
// with the call, putting the box in its layout if it is not there.
//
// Portaled to <body> like the station card; its content re-applies `--ui-zoom`. Esc closes it and is
// never prevented: it is the stop key too.
//
// Display only: nothing here transmits, tunes, turns or works anything.
//
// ⚠️ ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts).
import './prop/BestTimes.css'
import { useEffect, useLayoutEffect, useRef, useState, useSyncExternalStore, type CSSProperties } from 'react'
import { createPortal } from 'react-dom'
import { t } from '../i18n'
import type { BestMode, SpotRow } from '../types'
import { useUnits } from '../units'
import { lookupCall } from '../callsign'
import { closeCallCard, placeCallCard, txBoxes, type Placement } from './CallCard'
import { registerFloatingCard, setBestTimesOpener } from '../features/floatingCard'
import { planReturn, giveBack } from '../focusReturn'
import { BEST_TIMES, bestTimesKey, refreshKeyedFeed, useKeyedFeed } from '../features/connectFeeds'
import { CARD_WINDOWS, startMode } from '../features/bestTimes'
import { askBestTimes, handOff, lookUpOnce, lookedUp } from '../features/bestTimesAsk'
import { surfacePanel } from '../features/windowScope'
import { openPanelWindow } from '../api'
import { withErrorToast } from '../toast'
import { BestTimesAnswer } from './prop/BestTimesAnswer'

interface Opened {
  call: string
  anchor: HTMLElement
  keyboard: boolean
}

let opened: Opened | null = null
let hosts = 0
const listeners = new Set<() => void>()
const emit = () => listeners.forEach((l) => l())
const subscribe = (l: () => void) => {
  listeners.add(l)
  return () => {
    listeners.delete(l)
  }
}
const readOpened = () => opened

export function closeBestTimesCard(): void {
  if (!opened) return
  opened = null
  emit()
}

const closeOtherCards = registerFloatingCard(closeBestTimesCard)

/** Whether this window can show the card (a host is mounted). */
export function bestTimesCardHosted(): boolean {
  return hosts > 0
}

/** See full day: Conditions in its own window with the call's Best times box. In that window
 *  already, the box takes the call here. */
export function seeFullDay(call: string): void {
  closeBestTimesCard()
  if (surfacePanel() === 'connect') {
    askBestTimes(call)
    return
  }
  handOff(call)
  void withErrorToast(() => openPanelWindow('connect'), t('bestTimes.fullDay.failed'))
}

/** Open the card for `call` beside `anchor` (a call menu's item). Without a host: See full day. */
export function openBestTimesCard(call: string, anchor: HTMLElement, keyboard = false): void {
  const cu = call.trim().toUpperCase()
  if (!lookupCall(cu)) return
  if (hosts === 0) return seeFullDay(cu)
  closeOtherCards()
  closeCallCard()
  opened = { call: cu, anchor, keyboard }
  emit()
}
setBestTimesOpener((call, anchor) => openBestTimesCard(call, anchor))

function CardPopover({ open, spots, antGainDbi, radioMode }: { open: Opened; spots?: readonly SpotRow[]; antGainDbi?: number; radioMode?: string | null }) {
  const units = useUnits()
  const box = useRef<HTMLDivElement>(null)
  const inner = useRef<HTMLDivElement>(null)
  const [place, setPlace] = useState<Placement | null | undefined>(undefined)
  const [mode, setMode] = useState<BestMode>(() => startMode(radioMode))
  const [grid, setGrid] = useState<string | null>(null)
  const [lookingUp, setLookingUp] = useState(false)
  const [note, setNote] = useState<string | null>(null)
  const focusInside = useRef(false)
  const key = bestTimesKey(open.call, grid, mode)
  const view = useKeyedFeed(BEST_TIMES, key).value ?? null
  const nowUnix = Math.floor(Date.now() / 1000)

  const lookup = () => {
    setLookingUp(true)
    void lookUpOnce(open.call).then((placedAt) => {
      setLookingUp(false)
      if (!placedAt) setNote(t('bestTimes.lookup.none'))
      void refreshKeyedFeed(BEST_TIMES, key)
    })
  }
  // The menu item was an explicit ask: an answer from a country centre asks the callbook once.
  const entityOnly = view?.basis?.kind === 'entity' || view?.state === 'sameEntity'
  useEffect(() => {
    if (entityOnly && !lookedUp(open.call)) lookup()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [entityOnly, open.call])

  useLayoutEffect(() => {
    const replace = () => {
      if (!open.anchor.isConnected) return closeBestTimesCard()
      const a = open.anchor.getBoundingClientRect()
      const c = inner.current?.getBoundingClientRect()
      if (!c || !box.current) return
      const b = getComputedStyle(box.current)
      const bw = (parseFloat(b.borderLeftWidth) || 0) + (parseFloat(b.borderRightWidth) || 0)
      const bh = (parseFloat(b.borderTopWidth) || 0) + (parseFloat(b.borderBottomWidth) || 0)
      const p = placeCallCard(
        { left: a.left, top: a.top, right: a.right, bottom: a.bottom },
        { w: c.width + bw, h: c.height + bh },
        { w: window.innerWidth, h: window.innerHeight },
        txBoxes(),
      )
      // No room that keeps it off the transmit controls: See full day rather than nothing.
      if (p === null && c.width > 0) return seeFullDay(open.call)
      setPlace((old) => (old && p && old.left === p.left && old.top === p.top && old.maxHeight === p.maxHeight ? old : p))
    }
    replace()
    window.addEventListener('resize', replace)
    window.addEventListener('scroll', replace, true)
    const ro = typeof ResizeObserver === 'function' ? new ResizeObserver(replace) : null
    if (inner.current) ro?.observe(inner.current)
    return () => {
      window.removeEventListener('resize', replace)
      window.removeEventListener('scroll', replace, true)
      ro?.disconnect()
    }
  }, [open.anchor, open.call])

  const placed = !!place
  useEffect(() => {
    if (open.keyboard && placed) box.current?.focus({ preventScroll: true })
  }, [open.keyboard, placed])
  useEffect(() => {
    const anchor = open.anchor
    return () => {
      if (focusInside.current && anchor.isConnected) giveBack(planReturn(anchor, false))
    }
  }, [open])
  // Esc closes it (never prevented: it is the stop key too); a press outside it closes it.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') closeBestTimesCard()
    }
    const onDown = (e: PointerEvent) => {
      const at = e.target as Node | null
      if (at && (box.current?.contains(at) || open.anchor.contains(at))) return
      closeBestTimesCard()
    }
    window.addEventListener('keydown', onKey)
    document.addEventListener('pointerdown', onDown, true)
    return () => {
      window.removeEventListener('keydown', onKey)
      document.removeEventListener('pointerdown', onDown, true)
    }
  }, [open.anchor])

  const style: CSSProperties = place
    ? { left: place.left, top: place.top, maxWidth: place.width, maxHeight: place.maxHeight }
    : { left: 0, top: 0, visibility: 'hidden' }
  return createPortal(
    <div
      ref={box}
      className="callcard bt-card"
      role="dialog"
      aria-modal="false"
      aria-label={t('bestTimes.card.aria', { call: open.call })}
      data-placed={place ? 'yes' : 'no'}
      tabIndex={-1}
      style={style}
      onFocus={() => {
        focusInside.current = true
      }}
      onBlur={(e) => {
        if (!box.current?.contains(e.relatedTarget as Node | null)) focusInside.current = false
      }}
    >
      <div ref={inner} className="callcard-body" style={{ zoom: 'var(--ui-zoom, 1)' }}>
        {view && view.call === open.call ? (
          <BestTimesAnswer
            view={view}
            nowUnix={nowUnix}
            units={units}
            antGainDbi={antGainDbi}
            cap={CARD_WINDOWS}
            spots={spots}
            mode={mode}
            onMode={setMode}
            onLookup={lookedUp(open.call) ? undefined : lookup}
            lookingUp={lookingUp}
            lookupNote={note}
            onGrid={setGrid}
            onFullDay={() => seeFullDay(open.call)}
          />
        ) : (
          <div className="bt-answer">
            <span className="bt-call">{open.call}</span>
            <div className="pane-basic">{t('bestTimes.state.loading')}</div>
          </div>
        )}
      </div>
    </div>,
    document.body,
  )
}

/** The window's one Best times card. Mount it once per window (App, and every pop-out). */
export function BestTimesCardHost({ spots, antGainDbi, radioMode }: { spots?: readonly SpotRow[]; antGainDbi?: number; radioMode?: string | null }) {
  useEffect(() => {
    hosts++
    return () => {
      hosts--
      closeBestTimesCard()
    }
  }, [])
  const open = useSyncExternalStore(subscribe, readOpened)
  if (!open) return null
  return <CardPopover key={open.call} open={open} spots={spots} antGainDbi={antGainDbi} radioMode={radioMode} />
}
