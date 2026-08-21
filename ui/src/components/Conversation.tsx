// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). What does NOT come
// from the catalog: the CQ line itself (`CQ <MYCALL> <MYGRID>` is what goes on the air, and
// the two stand-ins below are what fills it before a callsign or grid is set), the band
// macros the operator typed, the peer's callsign, and the Winter Field Day chip — an event
// name, invariant exactly as it is in `FieldDayView.tsx`. The ⇄/⚙/💓/🤍 marks are glyphs.
//
// The Call CQ button and the heartbeat toggle put a signal on the air, but neither is a
// TX-enable latch and this cockpit renders no stop control (it has none — Tempo's stop line
// is the TopBar's, and APRS-style arming does not exist here), so nothing on this surface is
// on a stop-line census.
import type {
  ChatMessage,
  Conversation as Conv,
  FieldDayStatus,
  OpMode,
  RadioStatus,
  Settings,
} from '../types'
import { MessageBubble, type DeliveryStage } from './MessageBubble'
import { Composer } from './Composer'
import { usePinnedScroll } from '../usePinnedScroll'
import { t } from '../i18n'
import { T } from '../i18n/T'

/** What a CQ says before the station is set up. Both are on-air stand-ins, not prose. */
const CQ_PLACEHOLDER = { call: 'YOURCALL', grid: '----' }

/** Winter Field Day's own name, as the operator submits it — an event name, never translated. */
const WFD_CHIP = '🏕 Field Day · Winter'

interface Props {
  conversation: Conv | null
  peer: string | null
  radio: RadioStatus
  mode: OpMode
  fieldDay: FieldDayStatus | null
  macros: Settings['macros']
  onSend: (text: string) => void
  /** Tap-to-resend a terminal (no-ack / abandoned) bubble — re-queues the same text. */
  onResend?: (m: ChatMessage) => void
  /** Open broadcast (band chips) — free text, not directed at a peer. */
  onBroadcast: (text: string) => void
  /** Call CQ — sends ONE structured `CQ <call> <grid>` frame + arms TX (NOT a chunked
   * free-text broadcast). Distinct from onBroadcast so the CQ goes out clean. */
  onCallCq: () => void
  /** Presence heartbeat state + toggle — periodically beacons so listening stations can
   * receive your store-and-forward messages. */
  beaconOn: boolean
  onToggleBeacon: () => void
  mycall: string
  mygrid: string
  /** Roam (coordinated QSY) — a Tempo feature, living here now (was its own rail
   * section): the chip toggles it, the gear opens the full settings panel. All
   * optional so other Conversation hosts (detached windows) render unchanged. */
  roamEnabled?: boolean
  /** Short state for the chip while enabled ("20m", "paused"). */
  roamStatus?: string
  onToggleRoam?: () => void
  onRoamSettings?: () => void
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
  // BACKEND-TRUTH states first (the 2026-07 cadence rework made the engine authoritative):
  //   delivered = the id-bearing RR73 ACK came back — the only source of "Delivered ✓".
  //   no-ack    = sent its full cycle budget, never acknowledged. Terminal; tap to re-send.
  //   confirmed = the peer sent a COMPLETE directed message back after ours transmitted —
  //               they hear us, resends stopped. Shown as confirmed, never Delivered.
  if (m.delivered) return 'delivered'
  if (m.noAck) return 'no-ack'
  if (m.abandoned) return 'abandoned'
  if (m.stored) return 'held'
  if (m.confirmed) return 'confirmed'
  // Mid-schedule: released at least once, budget not exhausted → "sending, try k".
  if ((m.attempts ?? 0) > 0 && m.to != null) return 'sending'
  const isLastOutbound = conv.messages.slice(index + 1).every((x) => !x.outbound)
  if (isLastOutbound && transmitting) return 'on-air'
  // Sent but unacknowledged (incl. open broadcasts, which never get an ACK) — show a
  // single ✓ rather than a misleading "confirmed". (The old "a later inbound implies
  // they heard us" guess is gone: the backend's narrowed implicit-ACK replaces it.)
  return 'sent'
}

export function Conversation({
  conversation,
  peer,
  radio,
  mode,
  fieldDay,
  macros,
  onSend,
  onResend,
  onBroadcast,
  onCallCq,
  beaconOn,
  onToggleBeacon,
  mycall,
  mygrid,
  roamEnabled = false,
  roamStatus,
  onToggleRoam,
  onRoamSettings,
}: Props) {
  // Bottom-pinned via the shared discipline — this pane is a CHAT CLIENT
  // (Trillian ruling): pinned-at-bottom follows new messages; an operator
  // scrolled up reading history is NEVER yanked. The old snap was keyed on
  // message COUNT alone, which both yanked the reader on every inbound and
  // never re-ran on a peer switch — switching to a same-length conversation
  // opened it parked at the previous peer's offset. App now remounts this
  // component per peer (key=) so each conversation opens pinned to its newest
  // message. The pin is instant even though .message-scroll carries
  // `scroll-behavior: smooth` — the hook overrides it (a wheel gesture cancels
  // a smooth pin mid-animation).
  const pin = usePinnedScroll<HTMLDivElement>()

  // Winter-Field-Day-only FD chrome. Tempo (TempoFast chat) is a first-class Field Day
  // contact surface for WFD only (operator: TempoFast is relevant to Winter FD, not
  // Summer). Under SFD (event 'arrlfd') or no active FD this is null, so no FD
  // chrome renders anywhere in the Tempo cockpit. The narrowing (assign the
  // status, not a boolean) keeps `wfdFd` typed non-null where used.
  const wfdFd = fieldDay && fieldDay.event === 'wfd' ? fieldDay : null

  // TODO(FD parity §3.4): full digital-cockpit parity ("call CQ FD" as a
  // structured Msg::FieldDay CQ, and a one-tap "work this peer for FD" that runs
  // the FieldDayStation exchange over TempoFast and logs the QSO) needs a backend seam
  // that does not exist yet — onCallCq sends a generic CQ and there is no command
  // to run/complete an FD exchange with a chat peer. This surface is chrome +
  // exchange-chip only until that command lands; do NOT fabricate the API here.

  // No peer selected → the Call CQ launchpad: call CQ (a broadcast) to be heard
  // on the band without first picking a station, plus the editable band macros.
  if (!peer) {
    const cqText = `CQ ${mycall || CQ_PLACEHOLDER.call} ${mygrid || CQ_PLACEHOLDER.grid}`.trim()
    return (
      <section className="conversation panel empty-conv">
        <div className="empty-conv-inner">
          <h2>{t('tempo.empty.heading')}</h2>
          <p>{t('tempo.empty.body')}</p>
          {wfdFd && (
            <p className="conv-sub" style={{ marginBottom: 'var(--space-2)' }}>
              <T
                k="tempo.empty.fdActive"
                vals={{ event: WFD_CHIP }}
                tags={{
                  chip: (
                    <span
                      className="fd-state-chip"
                      style={{ borderColor: 'var(--accent)', color: 'var(--accent)' }}
                    />
                  ),
                }}
              />
            </p>
          )}
          <button type="button" className="cq-btn" onClick={onCallCq}>
            {t('tempo.cq.button')}
          </button>
          <p className="cq-onair">
            <T k="tempo.cq.onAir" vals={{ cq: cqText }} tags={{ b: <strong /> }} />
          </p>
          <button
            type="button"
            className={`heartbeat-btn${beaconOn ? ' on' : ''}`}
            onClick={onToggleBeacon}
            aria-pressed={beaconOn}
            title={t('tempo.heartbeat.launch.title')}
          >
            {beaconOn ? t('tempo.heartbeat.launch.on') : t('tempo.heartbeat.launch.off')}
          </button>
          {/* Roam must be reachable from the launchpad too — the conversation
              header (which also carries these chips) only exists once a peer is
              selected, and Roam setup usually happens BEFORE the QSO. */}
          {onToggleRoam && (
            <div className="empty-conv-roam">
              <button
                type="button"
                className={`heartbeat-btn roam-toggle${roamEnabled ? ' on' : ''}`}
                onClick={onToggleRoam}
                aria-pressed={roamEnabled}
                title={t('roam.chip.title')}
              >
                ⇄{' '}
                {roamEnabled
                  ? roamStatus
                    ? t('roam.chip.launch.on.status', { status: roamStatus })
                    : t('roam.chip.launch.on')
                  : t('roam.chip.launch.off')}
              </button>
              {onRoamSettings && (
                <button
                  type="button"
                  className="heartbeat-btn roam-gear"
                  onClick={onRoamSettings}
                  title={t('roam.chip.settings.title')}
                >
                  {t('roam.chip.settings.label')}
                </button>
              )}
            </div>
          )}
          <div className="quick-replies band-quickbar" aria-label={t('tempo.band.quickbar.aria')}>
            {macros.band.map((q, i) => (
              <button
                key={`${q}-${i}`}
                type="button"
                className="quick-chip"
                onClick={() => onBroadcast(q)}
              >
                {q}
              </button>
            ))}
          </div>
        </div>
      </section>
    )
  }

  const isBand = peer === '*'
  const messages = conversation?.messages ?? []

  return (
    <section className="conversation panel">
      <div className="panel-header conv-header">
        <h2 className="conv-peer">{isBand ? t('tempo.header.band') : peer}</h2>
        <span className="conv-sub">
          {isBand
            ? t('tempo.header.broadcastAs', { call: mycall || CQ_PLACEHOLDER.call })
            : t('tempo.header.messages', { count: messages.length })}
        </span>
        <button
          type="button"
          className={`heartbeat-chip${beaconOn ? ' on' : ''}`}
          onClick={onToggleBeacon}
          aria-pressed={beaconOn}
          title={t('tempo.heartbeat.chip.title')}
        >
          {beaconOn ? t('tempo.heartbeat.chip.on') : t('tempo.heartbeat.chip.off')}
        </button>
        {onToggleRoam && (
          <button
            type="button"
            className={`heartbeat-chip roam-toggle${roamEnabled ? ' on' : ''}`}
            onClick={onToggleRoam}
            aria-pressed={roamEnabled}
            title={t('roam.chip.title')}
          >
            ⇄{' '}
            {roamEnabled
              ? roamStatus
                ? t('roam.chip.label.status', { status: roamStatus })
                : t('roam.chip.label.on')
              : t('roam.chip.label')}
          </button>
        )}
        {onRoamSettings && (
          <button
            type="button"
            className="heartbeat-chip roam-gear"
            onClick={onRoamSettings}
            title={t('roam.chip.settings.title')}
            aria-label={t('roam.chip.settings.aria')}
          >
            ⚙
          </button>
        )}
        {wfdFd && (
          <>
            <span
              className="fd-state-chip"
              style={{ borderColor: 'var(--accent)', color: 'var(--accent)' }}
              title={t('tempo.header.fd.title')}
            >
              {WFD_CHIP}
            </span>
            <span className="conv-sub" title={wfdFd.state || undefined}>
              {wfdFd.dxcall
                ? t('tempo.header.fd.working', { call: wfdFd.dxcall })
                : wfdFd.running
                  ? t('tempo.header.fd.running')
                  : t('tempo.header.fd.searchPounce')}
            </span>
          </>
        )}
      </div>

      <div className="message-scroll" ref={pin.ref} onScroll={pin.onScroll}>
        {messages.length === 0 && <p className="empty">{t('tempo.messages.empty')}</p>}
        {messages.map((m, i) => (
          <MessageBubble
            key={`${m.slot}-${i}`}
            message={m}
            delivery={
              conversation
                ? deliveryStage(conversation, i, radio.transmitting)
                : undefined
            }
            onResend={onResend}
          />
        ))}
      </div>

      <Composer
        peer={peer}
        mode={mode}
        fieldDay={fieldDay}
        macros={macros}
        onSend={isBand ? onBroadcast : onSend}
        broadcast={isBand}
        mycall={mycall}
      />
    </section>
  )
}
