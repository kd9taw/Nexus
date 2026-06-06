import { useEffect, useRef, useState } from 'react'
import type { DecodeRow } from '../types'
import { StateBlock } from './StateBlock'

interface Props {
  /** This slot's decodes (the live per-slot feed from the snapshot). */
  decodes: DecodeRow[]
  /** Current slot index — used to age/sort accumulated history. */
  slot: number
  /** Session count of IR-HARQ rescues (decodes recovered by combining). */
  harqRescues: number
  /** Work / answer a decoded station. */
  onCall: (call: string) => void
}

/** A decode plus the slot it was last heard on (history bookkeeping). */
interface Entry extends DecodeRow {
  slot: number
}

type Filter = 'all' | 'cq' | 'me' | 'b4' | 'new'
type Sort = 'time' | 'snr' | 'freq'

/** Newest-first history cap. */
const MAX_HISTORY = 300

/**
 * Band Activity that ACCUMULATES across slots and freezes while you read it —
 * fixing WSJT-X's #1 UX complaint (a pane that auto-scrolls and resets every
 * cycle, so you can't read back or click a decode without it jumping).
 *
 * - History persists across RX slots (deduped by message+freq; a re-heard
 *   station updates its SNR + moves to the top).
 * - Freeze-on-hover: while the pointer is over the list it stops updating, so
 *   you can read/scroll/click; it resumes (and back-fills) on mouse-out.
 * - Filter (All / CQ / To me / B4 / New) and sort (time / SNR / freq).
 */
export function OperateDecodes({ decodes, slot, harqRescues, onCall }: Props) {
  const histRef = useRef<Map<string, Entry>>(new Map())
  const frozenRef = useRef<Entry[]>([])
  const [, setTick] = useState(0)
  const [frozen, setFrozen] = useState(false)
  const [filter, setFilter] = useState<Filter>('all')
  const [sort, setSort] = useState<Sort>('time')

  // Ingest this slot's decodes into the rolling history. Re-heard signals (same
  // message + ~freq) move to the newest position with their latest SNR.
  useEffect(() => {
    const m = histRef.current
    for (const d of decodes) {
      const key = `${d.message}|${Math.round(d.freqHz / 5)}`
      m.delete(key) // re-insert so Map order = recency
      m.set(key, { ...d, slot })
    }
    if (m.size > MAX_HISTORY) {
      const drop = m.size - MAX_HISTORY
      const it = m.keys()
      for (let i = 0; i < drop; i++) m.delete(it.next().value as string)
    }
    // Only re-render from new data when not frozen; while frozen the displayed
    // snapshot (frozenRef) stays put even though history keeps accumulating.
    if (!frozen) setTick((t) => t + 1)
  }, [decodes, slot, frozen])

  const computeList = (): Entry[] => {
    let list = Array.from(histRef.current.values())
    list = list.filter((d) => {
      switch (filter) {
        case 'cq':
          return d.isCq
        case 'me':
          return d.directedToMe
        case 'b4':
          return d.worked
        case 'new':
          return !d.worked
        default:
          return true
      }
    })
    list.sort((a, b) => {
      switch (sort) {
        case 'snr':
          return b.snr - a.snr
        case 'freq':
          return a.freqHz - b.freqHz
        default:
          return b.slot - a.slot // newest first
      }
    })
    return list
  }

  const list = frozen ? frozenRef.current : computeList()

  const onEnter = () => {
    frozenRef.current = computeList()
    setFrozen(true)
  }
  const onLeave = () => setFrozen(false)

  return (
    <section className="operate-decodes">
      <div className="od-head">
        <h2>Band Activity</h2>
        <div className="od-controls">
          <div className="od-filters" role="group" aria-label="Filter decodes">
            {(['all', 'cq', 'me', 'b4', 'new'] as Filter[]).map((f) => (
              <button
                key={f}
                type="button"
                className={`od-chip${filter === f ? ' active' : ''}`}
                aria-pressed={filter === f}
                onClick={() => setFilter(f)}
                title={FILTER_TITLE[f]}
              >
                {FILTER_LABEL[f]}
              </button>
            ))}
          </div>
          <label className="od-sort">
            <span className="od-sort-label">sort</span>
            <select value={sort} onChange={(e) => setSort(e.target.value as Sort)}>
              <option value="time">Time</option>
              <option value="snr">SNR</option>
              <option value="freq">Freq</option>
            </select>
          </label>
        </div>
      </div>

      <div className="od-status">
        <span className={`od-frozen${frozen ? ' on' : ''}`} aria-live="polite">
          {frozen ? '❄ frozen — release to resume' : `${list.length} heard`}
        </span>
        {harqRescues > 0 && (
          <span className="harq-chip" title={`IR-HARQ recovered ${harqRescues} decode(s) this session`}>
            HARQ ×{harqRescues}
          </span>
        )}
      </div>

      <div className="od-scroll" role="list" onMouseEnter={onEnter} onMouseLeave={onLeave}>
        {list.length === 0 && (
          <StateBlock
            kind="empty"
            title="No decodes yet"
            detail="Waiting for the next slot — decoded signals will appear here as they arrive."
          />
        )}
        {list.map((d, i) => (
          <div className={`decode-row ${rowClass(d)}`} role="listitem" key={`${d.message}-${d.freqHz}-${i}`}>
            <span className={`decode-tier ${d.tier.toLowerCase()}`} title={`Decoded by ${d.tier}`}>
              {d.tier}
            </span>
            <span className={`decode-snr ${snrClass(d.snr)}`}>{fmtSnr(d.snr)}</span>
            <span className="decode-freq">{Math.round(d.freqHz)}</span>
            <span className="decode-msg" title={d.message}>
              {d.message}
              {d.worked && <span className="b4-chip" title="Worked before">B4</span>}
              {d.isCq && !d.directedToMe && <span className="decode-tag cq">CQ</span>}
              {d.directedToMe && <span className="decode-tag me">YOU</span>}
              {d.rv > 0 && (
                <span className="harq-chip" title={`Recovered by IR-HARQ (RV0–RV${d.rv})`}>
                  HARQ·RV{d.rv}
                </span>
              )}
            </span>
            {d.from && (
              <button
                type="button"
                className="decode-work"
                onClick={() => onCall(d.from as string)}
                title={`Answer ${d.from}`}
              >
                {d.isCq ? 'Call' : 'Work'}
              </button>
            )}
          </div>
        ))}
      </div>
    </section>
  )
}

const FILTER_LABEL: Record<Filter, string> = {
  all: 'All',
  cq: 'CQ',
  me: 'To me',
  b4: 'B4',
  new: 'New',
}
const FILTER_TITLE: Record<Filter, string> = {
  all: 'All decodes',
  cq: 'CQ calls only',
  me: 'Directed to my callsign',
  b4: 'Worked before',
  new: 'Not worked before',
}

function rowClass(d: DecodeRow): string {
  if (d.directedToMe) return 'directed'
  if (d.worked) return 'worked'
  if (d.isCq) return 'cq'
  return 'new'
}

function fmtSnr(snr: number): string {
  return `${snr > 0 ? '+' : ''}${snr}`
}

function snrClass(snr: number): string {
  if (snr >= -10) return 'good'
  if (snr >= -18) return 'ok'
  return 'weak'
}
