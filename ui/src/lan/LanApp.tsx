// The Stations on this network window's page (src-tauri/src/remote_window.rs, `open_lan_stations_window`):
// the stations this computer is paired with over their own network, pairing with a new one, and
// the stream of the one the operator opens, with no internet (the operator's ruling of 2026-10-04,
// "Window only in v1"). Served from this computer's own loopback origin, so it reaches no Nexus
// command; everything it does goes on its one socket to this computer's Nexus, which holds the
// keys and the station records and does the cryptography (src-tauri/src/lan_client.rs).
import { useCallback, useEffect, useRef, useState } from 'react'
import { t } from '../i18n'
import { useViewport } from '../useViewport'
import { StreamView } from '../remote-web/StreamView'
import type { StreamEnvironment } from '../remote-web/stream-link'
import { LanConnection, lanStream } from './connection'
import { codeShaped, groupedKey, readTold, socketUrl, type ClosedReason, type ConnectReason, type LanFound, type LanStation, type PairReason } from './protocol'
import { lanFindLine, lanReachLine, lanUnreached } from '../remote-native/lanReach'
import '../remote-monitor/monitor.css'
import '../remote-web/remote.css'
import '../remote-web/remote-site.css'
import nexusMark from '../remote-web/nexus-mark.svg'

// Invariant tokens: the product name, and an address as a station shows one.
const BRAND = 'Nexus'
const EXAMPLE_ADDRESS = '192.168.1.20:42075'
// After control was taken back by the station (a decision about the hosted road there), the page
// acquires again once; not again within this long, so a station that keeps refusing is never asked
// in a loop.
const RESUME_EVERY_MS = 30_000
// And only if control shows free within this long of the stream's end: the lease's own heartbeat
// says so within a second or two.
const RESUME_WITHIN_MS = 10_000

/** The page's socket, as the browser gives it: what this page uses of it. */
export type PageSocket = {
  send: (text: string) => void
  close: () => void
  onopen: (() => void) | null
  onmessage: ((event: { data: unknown }) => void) | null
  onclose: (() => void) | null
}

/** Every reason this computer's Nexus can give, in a sentence. Literal calls, so the catalog
 *  guard can see each key. */
function sentence(reason: PairReason | ConnectReason | ClosedReason): string {
  switch (reason) {
    case 'badAddress': return t('lanWindow.reason.badAddress')
    case 'badCode': return t('lanWindow.reason.badCode')
    case 'badName': return t('lanWindow.reason.badName')
    case 'unreachable': return t('lanWindow.reason.unreachable')
    case 'pairingClosed': return t('lanWindow.reason.pairingClosed')
    case 'wrongCode': return t('lanWindow.reason.wrongCode')
    case 'stationProofFailed': return t('lanWindow.reason.stationProofFailed')
    case 'pairingFull': return t('lanWindow.reason.pairingFull')
    case 'stationUnavailable': return t('lanWindow.reason.stationUnavailable')
    case 'updateStation': return t('lanWindow.reason.updateStation')
    case 'updateComputer': return t('lanWindow.reason.updateComputer')
    case 'stationsFull': return t('lanWindow.reason.stationsFull')
    case 'storeUnavailable': return t('lanWindow.reason.storeUnavailable')
    case 'notStation': return t('lanWindow.reason.notStation')
    case 'unavailable': return t('lanWindow.reason.unavailable')
    case 'keyChanged': return t('lanWindow.reason.keyChanged')
    case 'notThisStation': return t('lanWindow.reason.notThisStation')
    case 'notPaired': return t('lanWindow.reason.notPaired')
    case 'unknownStation': return t('lanWindow.reason.unknownStation')
    case 'stationLeft': return t('lanWindow.reason.stationLeft')
    case 'connectionLost': return t('lanWindow.reason.connectionLost')
    case 'disconnected': return t('lanWindow.disconnected')
    // Nothing answered at the address: the card's own words for why (`tempo_stream::lan::unreached`).
    case 'otherNetwork':
    case 'refused':
    case 'noAnswer': return lanReachLine(reason)
  }
}

function openSocket(url: string): PageSocket {
  return new WebSocket(url) as unknown as PageSocket
}

type Road = { connection: LanConnection; station: LanStation }

export function LanApp({ open = openSocket, page = typeof location === 'undefined' ? '' : location.href, environment = lanStream }: {
  /** Where the page's socket comes from; a test's own. */
  open?: (url: string) => PageSocket
  /** This page's address: its socket is beside it. */
  page?: string
  /** The stream's browser; a test's own. */
  environment?: () => StreamEnvironment
}) {
  useViewport(1, true)
  const socket = useRef<PageSocket | null>(null)
  const [phase, setPhase] = useState<'opening' | 'open' | 'lost'>('opening')
  const [stations, setStations] = useState<LanStation[]>([])
  const [computer, setComputer] = useState('')
  const [storeFailed, setStoreFailed] = useState(false)
  const [pairing, setPairing] = useState(false)
  const [address, setAddress] = useState('')
  const [found, setFound] = useState<{ shacks: LanFound[]; available: boolean } | null>(null)
  const [code, setCode] = useState('')
  const [name, setName] = useState<string | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  const [said, setSaid] = useState<{ text: string; alert: boolean } | null>(null)
  const [road, setRoad] = useState<Road | null>(null)
  const current = useRef<Road | null>(null)
  const [forgetting, setForgetting] = useState<string | null>(null)
  const [another, setAnother] = useState<{ id: string; address: string } | null>(null)
  const [resume, setResume] = useState(0)
  const connecting = useRef<string | null>(null)

  const say = useCallback((value: Record<string, unknown>) => {
    try { socket.current?.send(JSON.stringify(value)) } catch { setPhase('lost') }
  }, [])

  useEffect(() => {
    const ws = open(socketUrl(page))
    socket.current = ws
    const leave = () => {
      current.current?.connection.dispose()
      current.current = null
      setRoad(null)
    }
    ws.onopen = () => { setPhase('open'); ws.send(JSON.stringify({ type: 'stations' })) }
    ws.onclose = () => { setPhase('lost'); setBusy(null); leave() }
    ws.onmessage = event => {
      if (typeof event.data !== 'string') return
      let told
      try { told = readTold(JSON.parse(event.data)) } catch { return }
      switch (told.type) {
        case 'stations':
          setStations(told.stations)
          setComputer(told.computer)
          setStoreFailed(told.error !== null)
          return
        case 'found':
          setFound({ shacks: told.shacks, available: told.available })
          return
        case 'paired':
          setBusy(null); setPairing(false); setCode('')
          setSaid({ text: t('lanWindow.pairedNow'), alert: false })
          say({ type: 'stations' })
          return
        case 'pairRefused':
          setBusy(null)
          setSaid({ text: sentence(told.reason), alert: true })
          return
        case 'connected': {
          const station = { id: told.road.stationId, addresses: [told.road.address], key: '' }
          const connection = new LanConnection(text => ws.send(text), told.road, environment())
          current.current = { connection, station }
          setBusy(null); setSaid(null); setAnother(null); setResume(0)
          setRoad(current.current)
          return
        }
        case 'connectRefused':
          setBusy(null)
          setSaid({ text: sentence(told.reason), alert: true })
          // Nothing answered where the station was: it may have moved, so offer to type where it is.
          if ((told.reason === 'unreachable' || lanUnreached(told.reason)) && connecting.current) setAnother({ id: connecting.current, address: '' })
          return
        case 'closed':
          leave()
          setSaid({ text: sentence(told.reason), alert: told.reason !== 'disconnected' })
          say({ type: 'stations' })
          return
        case 'answerRefused':
          current.current?.connection.answerRefused(told.reason)
          return
        case 'station':
          current.current?.connection.receive(told.message, new TextEncoder().encode(event.data).length)
      }
    }
    return () => { leave(); ws.onopen = ws.onclose = ws.onmessage = null; ws.close() }
  }, [open, page, say, environment])

  // Control taken back by the station as a side effect of a decision about the hosted road (Turn
  // Remote on or off there, revoking a browser), which gives this computer its grant back at once.
  // The stream ends `notController`, the lease's next heartbeat is refused, and the next state
  // shows control free: then the view starts again by itself, once, and acquires. Control held by
  // another device, or not allowed, is never fought for, and any other end stays an end until the
  // operator asks, as on the hosted road.
  const lastResume = useRef(-Infinity)
  useEffect(() => {
    if (!road) return
    const { stream: link, operations } = road.connection
    let wasRunning = false
    let pending: number | null = null
    const check = () => {
      if (pending === null) return
      const ops = operations.getSnapshot()
      if (performance.now() - pending > RESUME_WITHIN_MS) { pending = null; return }
      if (!ops.connected || !ops.fresh || !ops.state || ops.state.phase === 'controlling') return
      if (ops.state.phase === 'available' && ops.state.allowed) setResume(n => n + 1)
      pending = null
    }
    const offLink = link.subscribe(() => {
      const view = link.getSnapshot()
      if (view.phase === 'connecting' || view.phase === 'live' || view.phase === 'stalled') { wasRunning = true; return }
      if (view.phase !== 'ended' || !wasRunning) return
      wasRunning = false
      if (view.reason !== 'notController' || performance.now() - lastResume.current < RESUME_EVERY_MS) return
      lastResume.current = pending = performance.now()
      check()
    })
    const offOps = operations.subscribe(check)
    return () => { offLink(); offOps() }
  }, [road])

  // The pairing dialog's address field offers the stations a look by name finds (the operator's
  // ruling of 2026-10-04, "By name, or typed"); typing an address always works, found or not.
  const look = () => {
    setFound(null)
    say({ type: 'find' })
  }

  const connect = (id: string, typed?: string) => {
    connecting.current = id
    setBusy(`connect:${id}`)
    setSaid(null)
    say(typed ? { type: 'connect', stationId: id, address: typed } : { type: 'connect', stationId: id })
  }

  if (road) {
    return <StreamView key={resume} connection={road.connection} autostart
      station={t('lanWindow.stationAt', { address: road.station.addresses[0] })}
      disconnect={() => say({ type: 'disconnect' })} />
  }

  const shownName = name ?? computer
  const pairReady = !busy && phase === 'open' && address.trim() !== '' && codeShaped(code) && shownName.trim() !== ''
  return <div className="app remote-monitor-app remote-service-app remote-site">
    <header className="rm-header"><span className="remote-site-wordmark">
      <img className="remote-site-mark" src={nexusMark} alt="" width={28} height={28} />
      <strong>{BRAND}</strong>
    </span></header>
    <main className="rm-scroll" aria-label={t('lanWindow.title')}><div className="rm-content remote-account">
      <h1>{t('lanWindow.title')}</h1>
      <p className="remote-site-lead">{t('lanWindow.intro')}</p>
      {phase === 'opening' && <p className="remote-site-status" role="status">{t('lanWindow.opening')}</p>}
      {phase === 'lost' && <p className="rm-warning" role="alert">{t('lanWindow.lost')}</p>}
      {storeFailed && <p className="rm-warning" role="alert">{t('lanWindow.reason.storeUnavailable')}</p>}
      {said && <p className={said.alert ? 'rm-warning' : 'remote-site-status'} role={said.alert ? 'alert' : 'status'}>{said.text}</p>}
      <section className="rm-card remote-section">
        <h2>{t('lanWindow.paired')}</h2>
        {stations.length === 0 && <p>{t('lanWindow.none')}</p>}
        {stations.map(station => <div key={station.id} className="remote-station lan-station">
          <p><strong>{t('lanWindow.stationAt', { address: station.addresses[0] })}</strong></p>
          <p className="remote-site-status">{t('lanWindow.stationKey', { key: groupedKey(station.key) })}</p>
          {forgetting === station.id ? <>
            <p className="remote-warning">{t('lanWindow.forgetConfirm')}</p>
            <div className="remote-actions">
              <button type="button" className="remote-button" disabled={!!busy} onClick={() => { setForgetting(null); say({ type: 'forget', stationId: station.id }) }}>{t('lanWindow.forgetYes')}</button>
              <button type="button" className="remote-button remote-button--quiet" onClick={() => setForgetting(null)}>{t('lanWindow.keep')}</button>
            </div>
          </> : <div className="remote-actions">
            <button type="button" className="remote-button remote-button--primary" disabled={!!busy || phase !== 'open'} onClick={() => connect(station.id)}>
              {busy === `connect:${station.id}` ? t('lanWindow.connecting') : t('lanWindow.stream')}</button>
            <button type="button" className="remote-button remote-button--quiet" disabled={!!busy} onClick={() => setForgetting(station.id)}>{t('lanWindow.forget')}</button>
          </div>}
          {another?.id === station.id && <form onSubmit={event => { event.preventDefault(); if (another.address.trim()) connect(station.id, another.address.trim()) }}>
            <label>{t('lanWindow.otherAddress')}<input autoComplete="off" spellCheck={false} value={another.address} maxLength={21}
              onChange={event => setAnother({ id: station.id, address: event.target.value })} /></label>
            <span className="settings-hint">{t('lanWindow.addressHint', { example: EXAMPLE_ADDRESS })}</span>
            <button className="remote-button" disabled={!!busy || !another.address.trim()}>{t('lanWindow.stream')}</button>
          </form>}
        </div>)}
        {!pairing && <div className="remote-actions">
          <button type="button" className="remote-button" disabled={phase !== 'open'} onClick={() => { setPairing(true); setSaid(null); look() }}>{t('lanWindow.pair')}</button>
        </div>}
      </section>
      {pairing && <section className="rm-card remote-section remote-site-card--action" aria-label={t('lanWindow.pairTitle')}>
        <h2>{t('lanWindow.pairTitle')}</h2>
        <p>{t('lanWindow.pairHint')}</p>
        {/* Guarded on the submit too: Enter in a field submits a form whose button is disabled. */}
        <form onSubmit={event => {
          event.preventDefault()
          if (!pairReady) return
          setBusy('pair'); setSaid(null)
          say({ type: 'pair', address: address.trim(), code, name: shownName.trim() })
        }}>
          <label>{t('lanWindow.address')}<input autoComplete="off" spellCheck={false} value={address} maxLength={21} required
            onChange={event => setAddress(event.target.value)} /></label>
          <span className="settings-hint">{t('lanWindow.addressHint', { example: EXAMPLE_ADDRESS })}</span>
          {found === null
            ? <p className="remote-site-status" role="status">{t('lanWindow.finding')}</p>
            : found.shacks.length === 0
              ? <p className="settings-hint">{lanFindLine(found.available)}</p>
              : <div role="group" aria-labelledby="lan-found">
                <span id="lan-found" className="settings-hint">{t('lanWindow.found')}</span>
                <div className="remote-actions">{found.shacks.map(shack =>
                  <button type="button" key={shack.name} className="remote-button remote-button--quiet"
                    aria-pressed={address.trim() === shack.address} onClick={() => setAddress(shack.address)}>
                    {t('lanWindow.foundAt', { name: shack.name, address: shack.address })}</button>)}</div>
              </div>}
          {found?.available && <div className="remote-actions">
            <button type="button" className="remote-button remote-button--quiet" onClick={look}>{t('lanWindow.findAgain')}</button>
          </div>}
          <label>{t('lanWindow.code')}<input className="remote-site-code-input" autoComplete="off" spellCheck={false} value={code} maxLength={24} required
            onChange={event => setCode(event.target.value)} /></label>
          <span className="settings-hint">{t('lanWindow.codeHint')}</span>
          <label>{t('lanWindow.name')}<input autoComplete="off" value={shownName} maxLength={32} required
            onChange={event => setName(event.target.value)} /></label>
          <span className="settings-hint">{t('lanWindow.nameHint')}</span>
          <div className="remote-actions">
            <button className="remote-button remote-button--primary" disabled={!pairReady}>{busy === 'pair' ? t('lanWindow.pairing') : t('lanWindow.pairSubmit')}</button>
            <button type="button" className="remote-button remote-button--quiet" disabled={busy === 'pair'} onClick={() => setPairing(false)}>{t('lanWindow.cancel')}</button>
          </div>
        </form>
      </section>}
    </div></main>
  </div>
}
