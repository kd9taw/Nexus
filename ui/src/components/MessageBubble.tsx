import type { ChatMessage } from '../types'

interface Props {
  message: ChatMessage
  /** For outbound: how far through the delivery lifecycle. */
  delivery?: DeliveryStage
  /** Tap-to-resend for terminal bubbles (no-ack / abandoned): one click re-queues the
   * same text with a fresh cycle budget — no re-typing. */
  onResend?: (m: ChatMessage) => void
}

export type DeliveryStage =
  | 'abandoned'
  | 'held'
  | 'sending'
  | 'sent'
  | 'on-air'
  | 'confirmed'
  | 'delivered'
  | 'no-ack'

function techSubline(m: ChatMessage): string {
  const parts: string[] = []
  if (m.snr !== null && m.snr !== undefined) parts.push(`${m.snr > 0 ? '+' : ''}${m.snr} dB`)
  if (m.freqHz !== null && m.freqHz !== undefined) parts.push(`${m.freqHz} Hz`)
  if (m.dtSec !== null && m.dtSec !== undefined) parts.push(`dT ${m.dtSec.toFixed(1)}s`)
  if (m.tier) parts.push(m.tier)
  return parts.join(' · ')
}

function DeliveryTicks({
  stage,
  to,
  attempts,
}: {
  stage: DeliveryStage
  to?: string | null
  attempts?: number
}) {
  // 'held' names WHY it hasn't gone out — the operator can't tell a queued message from a
  // transmitted one otherwise, since every directed message goes via store-and-forward.
  const label =
    stage === 'abandoned'
      ? 'Not sent — abandoned on restart. Tap to send it again.'
      : stage === 'no-ack'
        ? `Sent ${attempts ?? '?'}× — no acknowledgement. Tap to send it again.`
        : stage === 'held'
          ? `Waiting to send${to ? ` — ${to} not heard yet` : ''}`
          : stage === 'sending'
            ? `Sending — try ${attempts ?? 1}`
            : stage === 'sent'
              ? 'Sent'
              : stage === 'on-air'
                ? 'On air'
                : stage === 'delivered'
                  ? 'Delivered' // a real id-bearing RR73 ACK came back
                  : 'Confirmed — they answered after this went out' // implicit, never "Delivered"
  return (
    <span className={`delivery ${stage}`} title={label} aria-label={label}>
      {stage === 'abandoned' && '⚠'}
      {stage === 'no-ack' && '⚠'}
      {stage === 'held' && '⋯'}
      {stage === 'sending' && `↻${attempts ?? ''}`}
      {stage === 'sent' && '✓'}
      {stage === 'on-air' && '✓✓'}
      {stage === 'confirmed' && '✓✓'}
      {stage === 'delivered' && '✓✓'}
    </span>
  )
}

export function MessageBubble({ message, delivery, onResend }: Props) {
  const side = message.outbound ? 'mine' : 'theirs'
  const sub = techSubline(message)
  const resendable =
    message.outbound && (delivery === 'no-ack' || delivery === 'abandoned') && onResend != null
  return (
    <div className={`bubble-row ${side}`}>
      <div
        className={`bubble ${side}${message.directedToMe ? ' directed' : ''}${resendable ? ' resendable' : ''}`}
        role={resendable ? 'button' : undefined}
        tabIndex={resendable ? 0 : undefined}
        title={resendable ? 'Tap to re-send this message' : undefined}
        onClick={resendable ? () => onResend(message) : undefined}
        onKeyDown={
          resendable
            ? (e) => {
                if (e.key === 'Enter' || e.key === ' ') onResend(message)
              }
            : undefined
        }
      >
        {!message.outbound && message.from && (
          <span className="bubble-from">{message.from}</span>
        )}
        <span className="bubble-text">{message.text}</span>
        <span className="bubble-meta">
          {sub && <span className="bubble-tech">{sub}</span>}
          {message.outbound && delivery && (
            <DeliveryTicks stage={delivery} to={message.to} attempts={message.attempts} />
          )}
        </span>
      </div>
    </div>
  )
}
