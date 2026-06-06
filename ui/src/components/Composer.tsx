import { useState } from 'react'
import type { FieldDayStatus, OpMode, Settings } from '../types'
import { clampToFrames } from '../freetext'
import { FreetextMeter } from './FreetextMeter'

interface Props {
  peer: string
  mode: OpMode
  fieldDay: FieldDayStatus | null
  macros: Settings['macros']
  onSend: (text: string) => void
}

// Mode-specific one-tap chips, sourced from the editable macros. Field Day's
// first chip is a dynamic exchange built from my class + section.
function quickRepliesFor(
  mode: OpMode,
  fieldDay: FieldDayStatus | null,
  macros: Settings['macros'],
): string[] {
  switch (mode) {
    case 'qso':
      return macros.qso
    case 'fieldDay': {
      const exchange =
        fieldDay && fieldDay.myClass && fieldDay.mySection
          ? `${fieldDay.myClass} ${fieldDay.mySection}`
          : null
      return exchange ? [exchange, 'RR73', '73'] : ['RR73', '73']
    }
    case 'chat':
    default:
      return macros.chat
  }
}

export function Composer({ peer, mode, fieldDay, macros, onSend }: Props) {
  const [text, setText] = useState('')
  const quickReplies = quickRepliesFor(mode, fieldDay, macros)

  const submit = (value: string) => {
    const v = value.trim()
    if (!v) return
    onSend(v)
    setText('')
  }

  return (
    <div className="composer">
      <div className="quick-replies" aria-label="Quick replies">
        {quickReplies.map((q, i) => (
          <button
            key={`${q}-${i}`}
            type="button"
            className="quick-chip"
            onClick={() => submit(q)}
          >
            {q}
          </button>
        ))}
      </div>
      <form
        className="composer-input-row"
        onSubmit={(e) => {
          e.preventDefault()
          submit(text)
        }}
      >
        <input
          className="composer-input"
          type="text"
          value={text}
          onChange={(e) => setText(clampToFrames(e.target.value))}
          placeholder={`Message ${peer}…`}
          aria-label={`Message ${peer}`}
          autoComplete="off"
        />
        <FreetextMeter text={text} />
        <button type="submit" className="send-btn" disabled={!text.trim()}>
          Send
        </button>
      </form>
    </div>
  )
}
