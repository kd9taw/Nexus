// The banner a computer shows while it drives a paired station (client mode): which station, how
// the link stands, and Leave.
//
// It renders nothing on a computer running its own station: the backend answers `null`, once, and
// the banner never asks again. Nor anywhere but the desktop app itself: the hosted page and the TV
// page mount the same App over transports that have no such question. While driving a station it
// asks every half second, because the link's state changes under it: connecting, live, not live
// (the panes keep the station's last state while Nexus connects again by itself), another device
// in control, or refused, with why.
//
// An in-flow strip, a child of `.app` like the other banners: it takes its own height from the
// shell below it and covers nothing, so no control of any cockpit, Stop included, is ever under it.
//
// It also says what the station last said of this computer's over (Stop's road): the holes on
// the air in it so far, in amber, why it ended, or that the station has gone quiet while a press
// is held. The short tone that goes with an end is played by Nexus itself, in this computer's
// headset.
//
// Over the internet road (Beta) it says so, and keeps the two things that road adds: "Still
// there?" after a quarter of an hour with no click, key or wheel here (each of which it reports,
// at most every few seconds, while the link is on the internet), with a button that answers it,
// and Reconnect once nobody did; and, when this network lets no datagram road through, that
// transmitting from here is off. A pop-out window reports its input the same way (`ClientInput`, at
// its root), so work in a pop-out answers "Still there?" too. A station signed in to from elsewhere
// (`away`: kept by its service id) has no address to name, so the banner says the
// internet while it connects and when the station does not take this computer.
//
// In a test build it carries the road switch too (`src-tauri/src/bench_road_switch.rs`): Normal,
// Block the command link, Block the Stop road, one click each, and a mark while a road is blocked.
// Only a test build's status names the switch, so a shipped build draws none of it.
//
// A station that no longer knows this computer, or answers with another key, is not fixed by waiting:
// it offers Pair again, which leaves and opens Stations on this network with the pairing for that
// station (`client_mode::pair_again`).
//
// While another device holds the station it offers Take control, asked inline (a modal would
// stand between the operator and Stop TX): confirmed, that device's over ends as its Stop would and
// this computer starts with TX off. A computer control was taken from says so, and can take it back
// the same way.

import { useEffect, useState, type Dispatch, type SetStateAction } from 'react'
import {
  benchRoadSwitch,
  clientStatus,
  clientStillHere,
  clientTakeControl,
  isTauri,
  leaveClientMode,
  type BenchPosition,
  type ClientOver,
  type ClientStatus,
  type PttRefusal,
} from '../api'
import { t } from '../i18n'
import { lanReachLine } from '../remote-native/lanReach'
import { heldLine } from '../features/txOnHeld'

/** How often the banner asks after the link while this computer drives a station. */
export const CLIENT_STATUS_EVERY_MS = 500
/** The operator's input is reported at most this often while the link is on the internet. */
export const STILL_HERE_EVERY_MS = 5_000

/** Why the station did not take this computer, in a sentence. Literal calls, so the catalog
 *  guard can see each key; a reason this build cannot name is said as such. A refusal for another
 *  build names both versions once the station's welcome has said its own. */
function reasonLine(status: ClientStatus): string {
  const builds =
    status.stationBuild != null && status.computerBuild != null
      ? { computer: status.computerBuild, station: status.stationBuild }
      : null
  if (builds) {
    switch (status.reason) {
      case 'updateStation': return t('client.reason.builds.updateStation', builds)
      case 'updateComputer': return t('client.reason.builds.updateComputer', builds)
      case 'otherBuild': return t('client.reason.builds.otherBuild', builds)
      default: break
    }
  }
  switch (status.reason) {
    case 'unreachable': return t('lanWindow.reason.unreachable')
    case 'otherNetwork':
    case 'refused':
    case 'noAnswer': return lanReachLine(status.reason)
    case 'keyChanged': return t('lanWindow.reason.keyChanged')
    case 'nameUnresolved': return t('lanWindow.reason.nameUnresolved')
    case 'notThisStation': return t('lanWindow.reason.notThisStation')
    case 'notPaired': return t('lanWindow.reason.notPaired')
    case 'notStation': return t('lanWindow.reason.notStation')
    case 'updateStation': return t('lanWindow.reason.updateStation')
    case 'updateComputer': return t('lanWindow.reason.updateComputer')
    case 'storeUnavailable': return t('lanWindow.reason.storeUnavailable')
    case 'unknownStation': return t('lanWindow.reason.unknownStation')
    case 'otherBuild': return t('client.reason.otherBuild')
    case 'localPermissionRequired': return t('client.reason.localPermissionRequired')
    case 'idle': return t('client.reason.idle')
    case 'internetNotIncluded': return t('client.reason.internetNotIncluded')
    case 'notApprovedThere': return t('client.reason.notApprovedThere')
    case 'waitingApproval': return t('client.reason.waitingApproval')
    case 'internetNotReady': return t('client.reason.internetNotReady')
    case 'stationUnavailable': return t('client.reason.stationUnavailable')
    case 'serviceUnavailable': return t('client.reason.serviceUnavailable')
    case 'tryLater': return t('client.reason.tryLater')
    case 'accessDenied': return t('client.reason.accessDenied')
    case 'pipeClosed': return t('client.reason.pipeClosed')
    case 'stationKeyUnconfirmed': return t('client.reason.stationKeyUnconfirmed')
    case 'stationKeyDisagrees': return t('client.reason.stationKeyDisagrees')
    default: return t('client.reason.unknown')
  }
}

/** Why an over ended, in a sentence. Literal calls, so the catalog guard can see each key. */
function endedLine(why: Extract<ClientOver, { phase: 'ended' }>['why']): string {
  switch (why) {
    case 'audioGap': return t('client.over.ended.audioGap')
    case 'pttGap': return t('client.over.ended.pttGap')
    case 'stopped': return t('client.over.ended.stopped')
    case 'presence': return t('client.over.ended.presence')
    case 'ceiling': return t('client.over.ended.ceiling')
    case 'watchdog': return t('client.over.ended.watchdog')
    case 'routeChanged': return t('client.over.ended.routeChanged')
    case 'refused': return t('client.over.ended.refused')
  }
}

/** Why a press of PTT here would not key the station, in a plain sentence. Literal calls, so the
 *  catalog guard can see each key. The station decides whether a press keys; this only explains a
 *  refusal the station reported, so the operator is not left pressing a dead key with no reason. */
function refusalLine(refusal: PttRefusal): string {
  switch (refusal) {
    case 'notPhone': return t('client.ptt.refusal.notPhone')
    case 'txDisabled': return t('client.ptt.refusal.txDisabled')
    case 'outsideLicence': return t('client.ptt.refusal.outsideLicence')
    case 'tuning': return t('client.ptt.refusal.tuning')
    case 'otherOwner': return t('client.ptt.refusal.otherOwner')
    case 'clockRepair': return t('client.ptt.refusal.clockRepair')
    case 'onHold': return t('client.ptt.refusal.onHold')
    case 'noPresence': return t('client.ptt.refusal.noPresence')
    case 'noFeed': return t('client.ptt.refusal.noFeed')
    case 'noAudio': return t('client.ptt.refusal.noAudio')
  }
}

/** This computer's over, in a sentence, when there is anything to say of it: an over on the air
 *  with no holes says nothing. The seconds are invariant, as every number on screen is. */
export function overLine(over: ClientOver | null | undefined): string | null {
  if (!over) return null
  switch (over.phase) {
    case 'onAir':
      return over.holes > 0
        ? t('client.over.gaps', { count: over.holes, seconds: (over.holeMs / 1000).toFixed(1) })
        : null
    case 'noWord': return t('client.over.noWord')
    case 'ended': return endedLine(over.why)
  }
}

function line(status: ClientStatus): string {
  const station = status.station
  if (status.phase === 'busy' && status.reason === 'takenOver') return t('client.banner.takenOver')
  if (status.internet) {
    switch (status.phase) {
      case 'live': return t('client.banner.internet.live')
      case 'notLive': return t('client.banner.internet.notLive')
      case 'busy': return t('client.banner.internet.busy')
      default: break
    }
  }
  // Signed in to from elsewhere: no address to name, and the internet its only road.
  if (status.away) {
    switch (status.phase) {
      case 'connecting': return t('client.banner.internet.connecting')
      case 'refused': return t('client.banner.internet.refused', { reason: reasonLine(status) })
      default: break
    }
  }
  switch (status.phase) {
    case 'connecting': return t('client.banner.connecting', { station })
    case 'live': return t('client.banner.live', { station })
    case 'notLive': return t('client.banner.notLive', { station })
    case 'busy': return t('client.banner.busy', { station })
    case 'refused':
      return t('client.banner.refused', { station, reason: reasonLine(status) })
  }
}

/** The road switch of a test build. Each move draws the switch's own answer at once; a move it
 *  refuses changes nothing, and the next ask after the link says where it is. A road can be
 *  blocked only while the other is open, as the switch itself allows. */
function RoadSwitch({
  position,
  moved,
}: {
  position: BenchPosition
  moved: (now: BenchPosition) => void
}) {
  const move = (to: BenchPosition) => {
    void benchRoadSwitch(to).then(moved, () => undefined)
  }
  return (
    <span className="client-banner-bench" role="group" aria-label={t('client.bench.label')}>
      <span className="client-banner-bench-label" aria-hidden="true">
        {t('client.bench.label')}
      </span>
      <button type="button" aria-pressed={position === 'normal'} onClick={() => move('normal')}>
        {t('client.bench.normal')}
      </button>
      <button
        type="button"
        aria-pressed={position === 'blockLink'}
        disabled={position === 'blockStopRoad'}
        onClick={() => move('blockLink')}
      >
        {t('client.bench.blockLink')}
      </button>
      <button
        type="button"
        aria-pressed={position === 'blockStopRoad'}
        disabled={position === 'blockLink'}
        onClick={() => move('blockStopRoad')}
      >
        {t('client.bench.blockStopRoad')}
      </button>
      {position === 'blockLink' && (
        <span className="client-banner-bench-mark">{t('client.bench.blocked.link')}</span>
      )}
      {position === 'blockStopRoad' && (
        <span className="client-banner-bench-mark">{t('client.bench.blocked.stopRoad')}</span>
      )}
    </span>
  )
}

/** How the link stands while this computer drives a station, asked every half second; `null` on a
 *  computer running its own station, which is asked once and never again. */
function useClientStatus(): [ClientStatus | null, Dispatch<SetStateAction<ClientStatus | null>>] {
  const [status, setStatus] = useState<ClientStatus | null>(null)
  useEffect(() => {
    if (!isTauri()) return
    let alive = true
    let timer: ReturnType<typeof setTimeout> | null = null
    const ask = async () => {
      const now = await clientStatus().catch(() => null)
      if (!alive) return
      setStatus(now)
      // A computer running its own station answers null: asked once, never again.
      if (now) timer = setTimeout(() => void ask(), CLIENT_STATUS_EVERY_MS)
    }
    void ask()
    return () => {
      alive = false
      if (timer) clearTimeout(timer)
    }
  }, [])
  return [status, setStatus]
}

/** On the internet road, the operator's own input in this window answers "Still there?" before it
 *  is asked: a click, a key or the wheel anywhere in it, reported at most every few seconds. */
function useStillHereInput(internet: boolean) {
  useEffect(() => {
    if (!internet) return
    let last = -Infinity
    const input = () => {
      const now = Date.now()
      if (now - last < STILL_HERE_EVERY_MS) return
      last = now
      void clientStillHere().catch(() => undefined)
    }
    const kinds = ['pointerdown', 'keydown', 'wheel'] as const
    for (const kind of kinds) window.addEventListener(kind, input, { capture: true, passive: true })
    return () => {
      for (const kind of kinds) window.removeEventListener(kind, input, { capture: true })
    }
  }, [internet])
}

/** A pop-out window's part in "Still there?": its input counts as the main window's does. It draws
 *  nothing; the prompt and its button stay with the banner in the main window. */
export function ClientInput() {
  const [status] = useClientStatus()
  useStillHereInput(status?.internet === true)
  return null
}

export function ClientBanner() {
  const [status, setStatus] = useClientStatus()
  const [leaving, setLeaving] = useState(false)
  const [confirming, setConfirming] = useState(false)
  useStillHereInput(status?.internet === true)
  // A question asked while another device held the station goes once it no longer does.
  const busy = status?.phase === 'busy'
  useEffect(() => {
    if (!busy) setConfirming(false)
  }, [busy])

  if (!status) return null
  const take = () => {
    setConfirming(false)
    void clientTakeControl().catch(() => undefined)
  }
  const over = overLine(status.over)
  const idleEnded = status.phase === 'refused' && status.reason === 'idle'
  const leave = (pairAgain?: boolean) => {
    setLeaving(true)
    void leaveClientMode(pairAgain).catch(() => setLeaving(false))
  }
  // Refused in a way only a new pairing ends. A station signed in to from elsewhere has no pairing
  // on this network to make again.
  const pairAgain = status.phase === 'refused' && !status.away
    && (status.reason === 'notPaired' || status.reason === 'keyChanged')
  const bench = status.bench
  return (
    <div
      className="client-banner"
      data-phase={status.phase}
      data-bench={bench}
      data-confirming={busy && confirming ? 'true' : undefined}
      role="status"
      aria-live="polite"
    >
      <span className="client-banner-line">{line(status)}</span>
      {over && (
        <span className="client-banner-over" data-over={status.over?.phase}>
          {over}
        </span>
      )}
      {/* Why a PTT press here would not key the station, from the client status. Beside the over
          line, the banner's own place for what the station says of transmitting; it never covers
          Stop. Absent on an older station build, so nothing shows then. */}
      {/* The station holds this computer from transmit, in any mode: Allow to transmit at the shack,
          with the minutes left only when the station sent them. It stands in for the PTT's own
          on-hold line, which says the same without the time. */}
      {status.txHold && (
        <span className="client-banner-over" data-tx-hold={status.txHold.leftSecs != null ? 'counting' : 'waiting'}>
          {heldLine(status.txHold)}
        </span>
      )}
      {status.pttRefusal && !(status.txHold && status.pttRefusal === 'onHold') && (
        <span className="client-banner-over" data-ptt-refusal={status.pttRefusal}>
          {refusalLine(status.pttRefusal)}
        </span>
      )}
      {status.udpBlocked && <span className="client-banner-over">{t('client.banner.udpBlocked')}</span>}
      {/* Signing in again before the station stops taking this computer on its network. The day is
          written YYYY-MM-DD, in UTC, never in a locale's format. */}
      {status.signingAgain
        ? <span className="client-banner-over">{t('client.banner.signingAgain')}</span>
        : status.signInBy != null && <span className="client-banner-over">
          {t('client.banner.signInBy', { date: new Date(status.signInBy * 1000).toISOString().slice(0, 10) })}</span>}
      {status.stillThere && <span className="client-banner-over">{t('client.banner.stillThere')}</span>}
      {(status.stillThere || idleEnded) && (
        <button type="button" className="client-banner-leave" onClick={() => void clientStillHere().catch(() => undefined)}>
          {idleEnded ? t('client.banner.connectAgain') : t('client.banner.stillHere')}
        </button>
      )}
      {busy && !confirming && (
        <button type="button" className="client-banner-leave" onClick={() => setConfirming(true)}>
          {t('client.banner.takeControl')}
        </button>
      )}
      {busy && confirming && (
        <span className="client-banner-confirm" role="group" aria-label={t('client.banner.takeControl')}>
          <span className="client-banner-over">{t('client.banner.takeControl.confirm')}</span>
          <button type="button" className="client-banner-leave" onClick={take}>
            {t('client.banner.takeControl')}
          </button>
          <button type="button" className="client-banner-leave" onClick={() => setConfirming(false)}>
            {t('client.banner.takeControl.cancel')}
          </button>
        </span>
      )}
      {bench && (
        <RoadSwitch
          position={bench}
          moved={(now) => setStatus((was) => (was ? { ...was, bench: now } : was))}
        />
      )}
      {pairAgain && (
        <button
          type="button"
          className="client-banner-leave"
          onClick={() => leave(true)}
          disabled={leaving}
          title={t('client.banner.pairAgain.title')}
        >
          {t('client.banner.pairAgain')}
        </button>
      )}
      <button
        type="button"
        className="client-banner-leave"
        onClick={() => leave()}
        disabled={leaving}
        title={t('client.banner.leave.title')}
      >
        {t('client.banner.leave')}
      </button>
    </div>
  )
}
