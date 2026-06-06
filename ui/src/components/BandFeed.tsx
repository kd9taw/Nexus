import { useEffect, useMemo, useRef, useState } from 'react'
import type { Conversation as Conv, Settings } from '../types'
import { clampToFrames } from '../freetext'
import { FreetextMeter } from './FreetextMeter'

interface Props {
  /** The "*" broadcast conversation, or null if none yet. */
  conversation: Conv | null
  mycall: string
  macros: Settings['macros']
  onBroadcast: (text: string) => void
}

function tech(snr: number | null, freqHz: number | null, tier: string | null): string {
  const parts: string[] = []
  if (snr !== null && snr !== undefined) parts.push(`${snr > 0 ? '+' : ''}${snr} dB`)
  if (freqHz !== null && freqHz !== undefined) parts.push(`${Math.round(freqHz)} Hz`)
  if (tier) parts.push(tier)
  return parts.join(' · ')
}

export function BandFeed({ conversation, mycall, macros, onBroadcast }: Props) {
  const [text, setText] = useState('')
  const scrollRef = useRef<HTMLDivElement>(null)

  const messages = useMemo(() => conversation?.messages ?? [], [conversation])
  // Broadcasts go out as `DE <MYCALL> <body>`, so that prefix counts against the
  // frame budget — the meter and cap account for it.
  const bcastPrefix = `DE ${mycall} `

  useEffect(() => {
    const el = scrollRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [messages.length])

  const send = (value: string) => {
    const v = value.trim()
    if (!v) return
    onBroadcast(v)
    setText('')
  }

  return (
    <section className="conversation panel band-feed">
      <div className="panel-header conv-header">
        <h2 className="conv-peer">Band Activity</h2>
        <span className="conv-sub">open broadcasts · heard by everyone</span>
      </div>

      <div className="band-scroll" ref={scrollRef}>
        {messages.length === 0 && (
          <p className="empty">No band activity yet — broadcast a CQ to everyone.</p>
        )}
        {messages.map((m, i) => (
          <div
            className={`band-row${m.outbound ? ' mine' : ''}`}
            key={`${m.slot}-${i}`}
          >
            <span className="band-from">{m.outbound ? mycall : (m.from ?? '???')}</span>
            <span className="band-text">{m.text}</span>
            {(() => {
              const sub = tech(m.snr, m.freqHz, m.tier)
              return sub ? <span className="band-tech">{sub}</span> : null
            })()}
          </div>
        ))}
      </div>

      <div className="composer band-composer">
        <div className="band-broadcast-note" role="note">
          <span className="band-broadcast-pill">BROADCAST</span>
          <span>Sends to <strong>everyone</strong> on frequency — not a directed message.</span>
        </div>
        <div className="quick-replies" aria-label="Broadcast quick replies">
          {macros.band.map((q, i) => (
            <button
              key={`${q}-${i}`}
              type="button"
              className="quick-chip"
              onClick={() => send(q)}
            >
              {q}
            </button>
          ))}
        </div>
        <form
          className="composer-input-row"
          onSubmit={(e) => {
            e.preventDefault()
            send(text)
          }}
        >
          <input
            className="composer-input"
            type="text"
            value={text}
            onChange={(e) => setText(clampToFrames(e.target.value, bcastPrefix))}
            placeholder="Broadcast to all stations…"
            aria-label="Broadcast to all stations"
            autoComplete="off"
          />
          <FreetextMeter text={text} prefix={bcastPrefix} />
          <button type="submit" className="send-btn broadcast" disabled={!text.trim()}>
            Broadcast
          </button>
        </form>
      </div>
    </section>
  )
}
