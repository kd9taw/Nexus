// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). The bubble's
// technical subline — SNR in dB, the audio frequency in Hz, dT in seconds and the tier the
// backend named — is a readout of wire values and stays in the code, as do the delivery
// glyphs (✓, ✓✓, ⚠, ⋯, ↻). What moved is what the operator READS: the delivery sentence
// behind each glyph, and the partial-message warning.
import type { ChatMessage } from '../types'
import { t } from '../i18n'

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
      ? t('tempo.bubble.abandoned')
      : stage === 'no-ack'
        ? t('tempo.bubble.noAck', { attempts: attempts ?? '?' })
        : stage === 'held'
          ? // Two whole messages: naming the peer is a different statement, not a tail.
            to
            ? t('tempo.bubble.held.peer', { call: to })
            : t('tempo.bubble.held')
          : stage === 'sending'
            ? t('tempo.bubble.sending', { attempt: attempts ?? 1 })
            : stage === 'sent'
              ? t('tempo.bubble.sent')
              : stage === 'on-air'
                ? t('tempo.bubble.onAir')
                : stage === 'delivered'
                  ? t('tempo.bubble.delivered') // a real id-bearing RR73 ACK came back
                  : t('tempo.bubble.confirmed') // implicit, never "Delivered"
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
        title={resendable ? t('tempo.bubble.resend.title') : undefined}
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
          {/* An inbound message that never fully arrived. Showing the fragments that DID land,
              plainly marked, beats the old behaviour: the message never appeared at all while
              its fragments sat visible in band activity, with nothing explaining the gap. */}
          {message.incomplete && (
            <span
              className="bubble-incomplete"
              title={t('tempo.bubble.incomplete.title', {
                got: message.incomplete[0],
                total: message.incomplete[1],
              })}
            >
              ⚠ {t('tempo.bubble.incomplete.badge', {
                got: message.incomplete[0],
                total: message.incomplete[1],
              })}
            </span>
          )}
          {sub && <span className="bubble-tech">{sub}</span>}
          {message.outbound && delivery && (
            <DeliveryTicks stage={delivery} to={message.to} attempts={message.attempts} />
          )}
        </span>
      </div>
    </div>
  )
}
