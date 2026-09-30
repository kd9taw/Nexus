// CONNECT'S SPOTS BOX — the Spots board in a Connect slot. It is the REAL board (SpotsPanel, the
// Spots view's own component, hosted as a pane) with the wiring its window gives the Spots view,
// so a row's Work is that view's act and nothing else: no second list, no second filter, no second
// work path. See `SpotsPane` for what a pane of the board keeps and leaves out; this one names no
// modes and follows no band, so it opens as the view does — every spot on the air, the default
// Heard-on-my-continent and Hide-worked filters on — in a filter copy of its own ('connect').
//
// ITS COLUMNS FOLLOW ITS OWN WIDTH. A Connect box can be a 200 px rail or a third of the bottom
// strip, where the board's nine columns cannot fit, so the box measures itself and stamps
// `data-fit` (s | m | l), and styles.css drops the columns a narrow box cannot hold. A measured
// attribute, never a query, for the reasons useRegionCols.ts gives: no size-based `@media` (it is
// zoom-blind) and no container queries (unverified on WebKitGTK and under this app's `zoom`).
// `clientWidth` is the box's own layout width in CSS px, the unit the templates are written in. Below
// STACK_BELOW it also stamps `data-stack`, and a row puts the call on a line of its own.
//
// It keys nothing. Working a row QSYs and opens a cockpit through the board's own handler, exactly
// as from the Spots view, and that handler keys no transmitter.
import { useLayoutEffect, useRef } from 'react'
import { SpotsPanel } from '../SpotsPanel'
import type { SpotsFeed } from './paneContext'
import { useStationControl } from '../../stationAccess'
import { CollectionStatus, useRemoteCollection } from '../../remote-web/collections'
import { t } from '../../i18n'

/** The box's filter copy — `nexus.spots.<key>.connect` (SpotsPane `scope`). */
const CONNECT_SPOTS_SCOPE = 'connect'

/** The three column sets, by the box's own width. Also the literal `data-fit` value. */
export type BoxFit = 's' | 'm' | 'l'

/**
 * The column set for a box `width` CSS px wide. Pure, so the thresholds test without layout.
 * Below 530 px: call, frequency and mode — a 200 px rail up to a 557 px one. Below 640: the age, the
 * entity and the comment come back. From 640: every column the board draws as a pane.
 * The middle set starts where every one of its headings fits whole in all five languages, measured
 * in Chrome: English from 450 px, Japanese 410, German and Spanish 510, French ("Commentaire") 530.
 * At 360, where it used to start, "Comment" was cut in every language.
 */
export function classifyBoxFit(width: number): BoxFit {
  if (width < 530) return 's'
  if (width < 640) return 'm'
  return 'l'
}

/**
 * Below this width (CSS px) a six-character call, the frequency and the mode no longer share a line,
 * so the row stacks: the dashboard rail's box at its floor is 175 px; a Connect box is never under 182.
 * Measured in Chrome.
 */
export const STACK_BELOW = 176

/** Keep `data-fit` (and `data-stack`) on the box in step with its own width. Stamped imperatively
 *  before paint, as useRegionCols stamps a region; a hidden box (0 wide) keeps what it last had. */
function useBoxFit<T extends HTMLElement>(): React.RefObject<T> {
  const ref = useRef<T>(null)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    let raf = 0
    const measure = () => {
      const w = el.clientWidth
      if (w >= 2) {
        el.setAttribute('data-fit', classifyBoxFit(w))
        el.toggleAttribute('data-stack', w < STACK_BELOW)
      }
    }
    measure()
    if (typeof ResizeObserver === 'undefined') return
    const ro = new ResizeObserver(() => {
      cancelAnimationFrame(raf)
      raf = requestAnimationFrame(measure)
    })
    ro.observe(el)
    return () => {
      cancelAnimationFrame(raf)
      ro.disconnect()
    }
  }, [])
  return ref
}

export function SpotsBox({ feed }: { feed: SpotsFeed }) {
  const ref = useBoxFit<HTMLDivElement>()
  // On the Remote page the list is the station's `spots` collection, as in the Phone cockpit's
  // Spots pane: the board once it is there, and the existing status while it is not.
  const control = useStationControl()
  const spotsRead = useRemoteCollection('spots')
  return (
    <div className="cn-spots" ref={ref}>
      {control || spotsRead?.phase === 'ready' ? (
        <SpotsPanel {...feed.board} spots={feed.rows} pane={{ scope: CONNECT_SPOTS_SCOPE }} />
      ) : (
        <p className="dim" role="status">{t('remote.spotsUnavailable')}</p>
      )}
      {!control && <CollectionStatus name="spots" />}
    </div>
  )
}
