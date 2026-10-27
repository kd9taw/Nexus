// Settings ▸ Remote access ▸ Devices: the shack's one list of everything that reaches this station, by
// person (the operator's ruling of 2026-10-25, which replaces the separate lists of browsers, computers,
// paired computers, people and the browser approvals under Advanced): you first, then each person invited,
// then any device added by a sign-in the station cannot yet tell whose.
//
// One row per computer, phone or browser, however many roads it takes: the station sends which ids are one
// device (`deviceRows`, what a Remove on any of them takes together), and anything it did not place is a row
// of its own. Each row shows how it connects, its state in plain words, and its actions; the technical
// details (keys, ids, how it was added, its approval's dates) fold under it.
//
// PRESENTATION ONLY. Every button runs the action the old lists ran, by the same ids: Approve (a device
// waiting, or one whose key is not pinned here) and Approve again (an approval ending) as before, Allow to
// transmit where `offersAllowTransmit` offers it, Remove as the list it came from removed it, the station
// controls switch, and Remove on a person (the invitation taken back). A Remove on a computer known only to
// this network goes through `removeLan`. Every one of them is refused through a stream, here (`RemoteStation.act`)
// and at the station: only the shack decides.
import { t } from '../i18n'
import { shortFingerprint } from '../remote-web/stream-protocol'
import { isNative, offersAllowTransmit, PLATFORM_NAMES, waitsForApprove, type AccountRole, type LanStatus,
  type RemoteStationAction, type RemoteStationStatus } from './types'

type Listed = RemoteStationStatus['devices'][number]
type Paired = LanStatus['devices'][number]
/** Whose a row is: yours (yours by sign-in, added at the shack, or paired by code), an invited person's, or
 *  added by a sign-in the station has not told whose yet. */
export type Person = { kind: 'you' } | { kind: 'guest'; mailbox: string; role?: AccountRole } | { kind: 'unknown' }
export type DeviceRow = { listed: Listed[]; lan: Paired[]; person: Person; live: boolean; holdLeft: number | null }
export type DeviceGroup = { person: Person; rows: DeviceRow[] }

// Literal `t('key')` calls, so the catalog's orphan check sees each one used.
const ROADS = { home: () => t('remote.devices.road.home'), internet: () => t('remote.devices.road.internet'),
  browser: () => t('remote.devices.road.browser') }
// Browser approval lifetime: the shack warns this long before the end that use cannot move.
const APPROVAL_WARNING_MS = 7 * 86400000
// A UTC calendar date, never locale formatting: a date an operator plans around must not read
// differently from one country to the next (the browser page does the same).
const utcDate = (ms: number) => new Date(ms).toISOString().slice(0, 10)

/** The rows, grouped by person: you first, then each person invited (in the order invited, with no devices
 *  too, so their invitation can be taken back here), then anyone else the station named, then the devices
 *  not known whose. */
export function deviceGroups(status: RemoteStationStatus | null | undefined): DeviceGroup[] {
  const devices = status?.devices ?? [], paired = status?.lan?.devices ?? []
  const people = status?.account?.people ?? []
  const guest = (mailbox: string): Person => ({ kind: 'guest', mailbox, role: people.find(p => p.mailbox === mailbox)?.role })
  const rows: DeviceRow[] = []
  const placedListed = new Set<string>(), placedLan = new Set<string>()
  for (const given of status?.deviceRows ?? []) {
    const listed = devices.filter(d => given.listed.includes(d.id) && !placedListed.has(d.id))
    const lan = paired.filter(c => given.lan.includes(c.id) && !placedLan.has(c.id))
    if (listed.length + lan.length === 0) continue
    listed.forEach(d => placedListed.add(d.id)); lan.forEach(c => placedLan.add(c.id))
    rows.push({ listed, lan, live: given.live, holdLeft: given.holdLeft ?? null,
      person: given.guest ? guest(given.guest) : given.unknown ? { kind: 'unknown' } : { kind: 'you' } })
  }
  // Anything the station did not place (an older station, or a list read in between): a row of its own.
  for (const d of devices.filter(d => !placedListed.has(d.id)))
    rows.push({ listed: [d], lan: [], live: status?.controller === d.id, holdLeft: null,
      person: d.addedBy === 'account' ? { kind: 'unknown' } : { kind: 'you' } })
  for (const c of paired.filter(c => !placedLan.has(c.id)))
    rows.push({ listed: [], lan: [c], live: status?.controller === c.id, holdLeft: null, person: { kind: 'you' } })
  const mailboxes = [...people.map(p => p.mailbox)]
  for (const row of rows) if (row.person.kind === 'guest' && !mailboxes.includes(row.person.mailbox)) mailboxes.push(row.person.mailbox)
  return [
    { person: { kind: 'you' } as Person, rows: rows.filter(row => row.person.kind === 'you') },
    ...mailboxes.map(mailbox => ({ person: guest(mailbox), rows: rows.filter(row => row.person.kind === 'guest' && row.person.mailbox === mailbox) })),
    { person: { kind: 'unknown' } as Person, rows: rows.filter(row => row.person.kind === 'unknown') },
  ].filter(group => group.person.kind !== 'unknown' || group.rows.length > 0)
}

/** How a row connects: this network (the LAN book), the internet (Nexus on a computer or phone), a browser. */
export function roadsOf(row: DeviceRow): ('home' | 'internet' | 'browser')[] {
  const roads: ('home' | 'internet' | 'browser')[] = []
  if (row.lan.length > 0) roads.push('home')
  if (row.listed.some(isNative)) roads.push('internet')
  if (row.listed.some(d => !isNative(d))) roads.push('browser')
  return roads
}

type Props = {
  status: RemoteStationStatus | null
  busy: boolean
  act: (action: RemoteStationAction) => void
  /** Remove a computer known only to this network: only at the shack, never through a stream. */
  removeLan: (deviceId: string) => void
  roleName: (role: AccountRole) => string
}

export function DevicesList({ status, busy, act, removeLan, roleName }: Props) {
  const groups = deviceGroups(status)
  const account = status?.account
  // Approved, with a key the service lists, and that key not the one pinned here (approved before keys, or
  // it came back with a new one): it cannot connect until approved again.
  const unpinned = (d: Listed) => d.approved === 1 && !!d.key && !status?.pinnedDevices?.includes(d.id)
  const asks = (d: Listed) => d.approved !== 1 || waitsForApprove(status, d.id)
  const ending = (d: Listed) => d.approved === 1 && d.untilRemoved !== 1 && (d.renewsUntil ?? d.expiresAt) - Date.now() <= APPROVAL_WARNING_MS
  const approve = (d: Listed) => act({ type: 'device', deviceId: d.id, approve: true, key: d.key ?? undefined, kind: d.kind ?? 'browser' })
  const badge = (road: 'home' | 'internet' | 'browser') => ROADS[road]()
  function state(row: DeviceRow, role: AccountRole | undefined): string[] {
    const said: string[] = []
    if (row.live) said.push(t('remote.devices.state.live'))
    const waiting = row.listed.filter(asks)
    for (const d of waiting) said.push(isNative(d) ? t('remote.native.computerAsks') : t('remote.native.asks'))
    const held = row.listed.filter(d => offersAllowTransmit(status, d))
    if (held.length > 0) said.push(row.holdLeft ? t('remote.devices.state.heldFor', { minutes: Math.max(1, Math.ceil(row.holdLeft / 60)) })
      : t('remote.account.waitingToTransmit'))
    if (waiting.length === 0 && held.length === 0 && !row.listed.some(unpinned)) {
      const browserOnly = row.lan.length === 0 && row.listed.every(d => !isNative(d))
      said.push(browserOnly ? t('remote.devices.state.approved')
        : role === 'listen' ? t('remote.devices.state.listenOnly') : t('remote.devices.state.allowed'))
    }
    return said
  }
  function remove(row: DeviceRow) {
    // As the list it came from removed it: a device the service lists by its id there (every road the station
    // links to it goes with it), else a computer known only to this network by its LAN id.
    const listed = row.listed.find(d => d.approved === 1)
    if (listed) return <button type="button" className="remote-button" disabled={busy}
      onClick={() => act({ type: 'device', deviceId: listed.id, approve: false })}>{t('remote.native.remove')}</button>
    const lan = row.lan[0]
    if (lan) return <button type="button" className="remote-button" disabled={busy}
      onClick={() => removeLan(lan.id)}>{t('remote.native.remove')}</button>
    return null
  }
  function details(row: DeviceRow) {
    return <details className="remote-device-details"><summary>{t('remote.devices.details')}</summary>
      <ul className="remote-device-facts">
        {row.lan.map(c => <li key={`lan-${c.id}`}>
          <span>{t('remote.devices.addedLan', { road: badge('home') })}</span>{' '}
          <span className="remote-native-browser-key">{t('remote.browserKey', { key: shortFingerprint(c.key) })}</span>{' '}
          <code>{c.id.slice(-6)}</code>
        </li>)}
        {row.listed.map(d => <li key={`listed-${d.id}`}>
          <span>{badge(isNative(d) ? 'internet' : 'browser')}{d.platform ? ` · ${t('remote.native.computerPlatform', { platform: PLATFORM_NAMES[d.platform] })}` : ''}</span>{' '}
          {d.key && <span className="remote-native-browser-key">{t('remote.browserKey', { key: shortFingerprint(d.key) })}</span>}{' '}
          <code>{d.id.slice(-6)}</code>
          <span>{d.addedBy === 'account' ? t('remote.account.addedByAccount') : t('remote.devices.addedHere')}</span>
          {d.approved === 1 && <span>{d.untilRemoved === 1 ? t('remote.browserApprovedUntilRemoved') : d.renewsUntil
            ? t('remote.browserRenewsUntil', { until: utcDate(d.expiresAt), limit: utcDate(d.renewsUntil) })
            : t('remote.browserApprovedUntil', { until: utcDate(d.expiresAt) })}</span>}
          {/* The restrict switch: station controls for an approved browser, as under Advanced before. */}
          {!isNative(d) && d.approved === 1 && status?.stationPermissions && <button type="button" className="remote-button"
            disabled={busy || !['connected', 'connecting', 'reconnecting'].includes(status.phase)}
            onClick={() => act({ type: 'stationPermission', deviceId: d.id, allow: !status.stationPermissions!.includes(d.id) })}>
            {status.stationPermissions.includes(d.id) ? t('remote.controlRevoke') : t('remote.controlAllow')}</button>}
        </li>)}
      </ul>
    </details>
  }
  function rowOf(row: DeviceRow, role: AccountRole | undefined) {
    const first = row.listed[0] ?? row.lan[0]
    const key = `${row.listed.map(d => d.id).join()}|${row.lan.map(c => c.id).join()}`
    const platform = row.listed.find(d => d.platform)?.platform
    return <li key={key} className="remote-native-browser remote-device">
      <span className="remote-native-browser-name">{first.name}</span>
      {platform && <span>{t('remote.native.computerPlatform', { platform: PLATFORM_NAMES[platform] })}</span>}
      <span className="remote-device-roads">{roadsOf(row).map(road => <span key={road} className="remote-device-road">{badge(road)}</span>)}</span>
      <span className="remote-native-browser-state">{state(row, row.listed.find(d => d.role)?.role ?? role).join(' ')}</span>
      {/* What the operator must weigh before pressing Approve stays on the row: the key it pins, and why it asks. */}
      {row.listed.filter(d => (asks(d) || unpinned(d)) && d.key).map(d =>
        <span key={`key-${d.id}`} className="remote-native-browser-key">{t('remote.browserKey', { key: shortFingerprint(d.key!) })}</span>)}
      {row.listed.filter(d => unpinned(d) && !waitsForApprove(status, d.id)).map(d => <span key={`pin-${d.id}`} className="remote-warning">
        {isNative(d) ? t('remote.native.computerKeyNotPinned') : t('remote.browserKeyNotPinned')}</span>)}
      {row.listed.filter(d => account?.unconfirmed?.[d.id] && !waitsForApprove(status, d.id)).map(d =>
        <span key={`unconfirmed-${d.id}`} className="remote-warning">{t('remote.account.unconfirmed')}</span>)}
      {row.listed.filter(d => account?.removals?.includes(d.id)).map(d => <span key={`removal-${d.id}`}>{t('remote.account.removalWaiting')}</span>)}
      {row.listed.filter(ending).map(d => <span key={`ending-${d.id}`} className="remote-warning">
        {t('remote.browserApprovalEnding', { until: utcDate(d.renewsUntil ?? d.expiresAt) })}</span>)}
      <span className="remote-actions">
        {row.listed.filter(d => asks(d) || unpinned(d)).map(d => <button key={`approve-${d.id}`} type="button" className="remote-button"
          disabled={busy} onClick={() => approve(d)}>{t('remote.approve')}</button>)}
        {row.listed.filter(d => ending(d) && !unpinned(d)).map(d => <button key={`again-${d.id}`} type="button" className="remote-button"
          disabled={busy} onClick={() => approve(d)}>{t('remote.approveAgain')}</button>)}
        {row.listed.filter(d => offersAllowTransmit(status, d)).map(d => <button key={`allow-${d.id}`} type="button" className="remote-button"
          disabled={busy} onClick={() => act({ type: 'allowTransmit', deviceId: d.id })}>{t('remote.account.allowTransmit')}</button>)}
        {remove(row)}
      </span>
      {details(row)}
    </li>
  }
  return <section className="remote-devices" aria-labelledby="remote-devices-title">
    <h3 id="remote-devices-title">{t('remote.devices.title')}</h3>
    <p className="settings-hint">{t('remote.devices.hint')}</p>
    {groups.map(group => {
      const person = group.person
      const name = person.kind === 'you' ? t('remote.devices.you') : person.kind === 'guest' ? person.mailbox : t('remote.devices.unknown')
      const role = person.kind === 'guest' ? person.role : undefined
      return <div key={person.kind === 'guest' ? `guest-${person.mailbox}` : person.kind} className="remote-device-person" role="group" aria-label={name}>
        <h4 className="remote-device-person-name">
          <span>{name}</span>
          {role && <span className="remote-device-person-role">{roleName(role)}</span>}
          {/* The invitation taken back: the same Remove People had. */}
          {person.kind === 'guest' && role && <span className="remote-actions"><button type="button" className="remote-button" disabled={busy}
            onClick={() => act({ type: 'uninvite', email: person.mailbox })}>{t('remote.account.uninvite')}</button></span>}
        </h4>
        {group.rows.length === 0 ? <p className="settings-hint">{t('remote.devices.none')}</p>
          : <ul className="remote-native-browsers">{group.rows.map(row => rowOf(row, role))}</ul>}
      </div>
    })}
  </section>
}
