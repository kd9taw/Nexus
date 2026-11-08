// THE BEST TIMES BOX (Conditions): a call box and a Best times button, then the answer for that call —
// now, the next best windows, the week — beside a band-by-UTC-hour chart. Side by side once the box is
// SIDE_BY_SIDE_PX wide (measured on the box itself, never the window), else the answer above the chart;
// the preset rails are narrower than that, so a rail shows them stacked until it is dragged wider.
// Long content scrolls in the box's own body: the frame sizes the box, never this.
//
// It follows only its own box, a call menu's Best times item and a card's See full day
// (features/bestTimesAsk) — never the selected station. With an answer resting on a country centre
// after an explicit ask, it asks the callbook once and asks again (features/bestTimesAsk).
//
// Display only: nothing here transmits, tunes, turns or works anything.
import './BestTimes.css'
import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import type { BestMode, SpotRow } from '../../types'
import { t } from '../../i18n'
import type { Units } from '../../units'
import { BEST_TIMES, bestTimesKey, refreshKeyedFeed, useKeyedFeed } from '../../features/connectFeeds'
import { SIDE_BY_SIDE_PX, startMode } from '../../features/bestTimes'
import { lastAsked, lookUpOnce, lookedUp, rememberAsked, subscribeAsk, takeAsk } from '../../features/bestTimesAsk'
import { surfaceGet, surfaceSet } from '../../features/windowScope'
import { BestTimesAnswer } from './BestTimesAnswer'
import { BestTimesChart } from './BestTimesChart'

const MODE_KEY = 'nexus.bestTimes.mode'

/** The box's width class, measured on the box: a hidden box (0 wide) keeps what it had. */
function useWide<T extends HTMLElement>(): [React.RefObject<T>, boolean] {
  const ref = useRef<T>(null)
  const [wide, setWide] = useState(false)
  useLayoutEffect(() => {
    const el = ref.current
    if (!el) return
    const measure = () => {
      const w = el.clientWidth
      if (w >= 2) setWide(w >= SIDE_BY_SIDE_PX)
    }
    measure()
    if (typeof ResizeObserver === 'undefined') return
    let raf = 0
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
  return [ref, wide]
}

export function BestTimesPane({
  units,
  antGainDbi,
  spots,
  radioMode,
}: {
  units: Units
  antGainDbi?: number
  spots?: readonly SpotRow[]
  /** The radio's mode now, which the mode chips start from. */
  radioMode?: string | null
}) {
  const [ref, wide] = useWide<HTMLDivElement>()
  const [text, setText] = useState(lastAsked)
  const [call, setCall] = useState(lastAsked)
  const [grids, setGrids] = useState<Record<string, string>>({})
  const [mode, setModeState] = useState<BestMode>(() => {
    const kept = surfaceGet(MODE_KEY)
    return kept ? startMode(kept) : startMode(radioMode)
  })
  /** The operator asked explicitly (pressed the button, chose a menu item): a lookup may follow. */
  const [explicit, setExplicit] = useState(false)
  const [lookingUp, setLookingUp] = useState(false)
  const [note, setNote] = useState<string | null>(null)
  const [nowUnix, setNowUnix] = useState(() => Math.floor(Date.now() / 1000))
  useEffect(() => {
    const id = setInterval(() => setNowUnix(Math.floor(Date.now() / 1000)), 60_000)
    return () => clearInterval(id)
  }, [])

  const ask = (c: string, byHand: boolean) => {
    const cu = c.trim().toUpperCase()
    setText(cu)
    setCall(cu)
    setExplicit(byHand)
    setNote(null)
    rememberAsked(cu)
  }
  // A call handed over by a call menu or a card.
  useEffect(() => {
    const take = () => {
      const c = takeAsk()
      if (c) ask(c, true)
    }
    take()
    return subscribeAsk(take)
  }, [])
  const setMode = (m: BestMode) => {
    setModeState(m)
    surfaceSet(MODE_KEY, m)
  }
  const grid = grids[call] ?? null
  const key = call ? bestTimesKey(call, grid, mode) : null
  const view = useKeyedFeed(BEST_TIMES, key).value ?? null
  const shown = view && view.call === call.replace(/\s+/g, '') ? view : null

  const lookup = () => {
    if (!call) return
    setLookingUp(true)
    void lookUpOnce(call).then((placed) => {
      setLookingUp(false)
      if (!placed) setNote(t('bestTimes.lookup.none'))
      if (key) void refreshKeyedFeed(BEST_TIMES, key)
    })
  }
  // After an explicit ask, an answer from a country centre asks the callbook once, on its own.
  const entityOnly = shown?.basis?.kind === 'entity' || shown?.state === 'sameEntity'
  useEffect(() => {
    if (explicit && entityOnly && call && !lookedUp(call)) lookup()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [explicit, entityOnly, call])

  return (
    <div ref={ref} className={`best-times${wide ? ' wide' : ''}`}>
      <form
        className="bt-ask"
        onSubmit={(e) => {
          e.preventDefault()
          ask(text, true)
        }}
      >
        <input
          className="bt-input"
          aria-label={t('bestTimes.input.label')}
          placeholder={t('bestTimes.input.placeholder')}
          value={text}
          onChange={(e) => setText(e.target.value)}
          spellCheck={false}
          autoComplete="off"
          autoCapitalize="characters"
        />
        <button type="submit" className="bt-go">
          {t('bestTimes.go')}
        </button>
      </form>
      {!call && <div className="pane-basic">{t('connect.pane.bestTimes.basic')}</div>}
      {call && !shown && <div className="pane-basic">{t('bestTimes.state.loading')}</div>}
      {shown && (
        <div className="bt-body">
          <BestTimesAnswer
            view={shown}
            nowUnix={nowUnix}
            units={units}
            antGainDbi={antGainDbi}
            spots={spots}
            mode={mode}
            onMode={setMode}
            onLookup={lookedUp(call) ? undefined : lookup}
            lookingUp={lookingUp}
            lookupNote={note}
            onGrid={(g) => setGrids((m) => ({ ...m, [call]: g }))}
          />
          {shown.answer && <BestTimesChart answer={shown.answer} />}
        </div>
      )}
    </div>
  )
}
