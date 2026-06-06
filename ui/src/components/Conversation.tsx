import { useEffect, useRef } from 'react'
import type {
  Conversation as Conv,
  FieldDayStatus,
  OpMode,
  RadioStatus,
  Settings,
} from '../types'
import { MessageBubble, type DeliveryStage } from './MessageBubble'
import { Composer } from './Composer'

interface Props {
  conversation: Conv | null
  peer: string | null
  radio: RadioStatus
  mode: OpMode
  fieldDay: FieldDayStatus | null
  macros: Settings['macros']
  peerTyping: boolean
  onSend: (text: string) => void
}

/**
 * Derive a delivery stage for an outbound bubble. The newest outbound message
 * is "on-air" while we're transmitting and "confirmed" once a later inbound
 * message arrives; older outbound messages are treated as confirmed.
 */
function deliveryStage(
  conv: Conv,
  index: number,
  transmitting: boolean,
): DeliveryStage | undefined {
  const m = conv.messages[index]
  if (!m.outbound) return undefined
  const isLastOutbound =
    conv.messages.slice(index + 1).every((x) => !x.outbound)
  const hasLaterInbound = conv.messages
    .slice(index + 1)
    .some((x) => !x.outbound)
  if (hasLaterInbound) return 'confirmed'
  if (isLastOutbound && transmitting) return 'on-air'
  if (isLastOutbound) return 'sent'
  return 'confirmed'
}

export function Conversation({
  conversation,
  peer,
  radio,
  mode,
  fieldDay,
  macros,
  peerTyping,
  onSend,
}: Props) {
  const scrollRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    const el = scrollRef.current
    if (el) el.scrollTop = el.scrollHeight
  }, [conversation?.messages.length, peerTyping])

  if (!peer) {
    return (
      <section className="conversation panel empty-conv">
        <div className="empty-conv-inner">
          <h2>No conversation selected</h2>
          <p>Pick a station from the roster to start a QSO.</p>
        </div>
      </section>
    )
  }

  const messages = conversation?.messages ?? []

  return (
    <section className="conversation panel">
      <div className="panel-header conv-header">
        <h2 className="conv-peer">{peer}</h2>
        <span className="conv-sub">{messages.length} messages</span>
      </div>

      <div className="message-scroll" ref={scrollRef}>
        {messages.length === 0 && (
          <p className="empty">No messages yet — say hello.</p>
        )}
        {messages.map((m, i) => (
          <MessageBubble
            key={`${m.slot}-${i}`}
            message={m}
            delivery={
              conversation
                ? deliveryStage(conversation, i, radio.transmitting)
                : undefined
            }
          />
        ))}
        {peerTyping && (
          <div className="bubble-row theirs">
            <div className="bubble theirs typing">
              <span className="typing-label">{peer} is sending</span>
              <span className="typing-dots">
                <i />
                <i />
                <i />
              </span>
            </div>
          </div>
        )}
      </div>

      <Composer peer={peer} mode={mode} fieldDay={fieldDay} macros={macros} onSend={onSend} />
    </section>
  )
}
