/**
 * Settings ▸ Radio ▸ Transmit limits & sharing ▸ Share by TCI: Nexus as a TCI server for the
 * programs on THIS computer (WSJT-X, JTDX, MSHV, loggers, amplifier and tuner tools), beside the
 * CAT broker's "Share this radio with other programs". Both may be on.
 *
 * What the words below promise, and the code that keeps each promise:
 * - Only programs on this computer connect: the server listens on 127.0.0.1 and ::1 and refuses
 *   any handshake a web page makes (`tempo_net::tci::server`). So there is no address to set and
 *   no network choice here.
 * - THE ADDRESS IS THE STATION'S, NEVER THE FORM'S: the port the server bound
 *   (`radio.tciServer.port`). A second Nexus window serves on the next port while its setting still
 *   names the first. A port it could not bind shows the station's own sentence in the address's
 *   place, and no Copy: an address nothing listens on is worse than none (the CAT broker's rule).
 * - ⛔ "Programs on TCI may transmit" is drawn and set at the station only. From a computer driving
 *   the station its switch is not drawn at all; the row says "Set at the station", and a save from
 *   there keeps the station's value (`keep_the_shacks_transmit_settings`,
 *   src-tauri/src/command_dispatch.rs). Off as it comes. With it on, the engine's TCI rules decide
 *   each key behind every Nexus transmit gate (`tempo_app::engine::tci`), and FT8, FT4 and the
 *   other slot-timed modes stay refused until the delay Nexus adds has been measured: said beside
 *   the switch, wherever the row is drawn.
 * - Turning the server off ends every program's connection, and turning "may transmit" off ends
 *   every program's key: the station does both when the save lands (src-tauri/src/tci_server.rs,
 *   `Engine::apply_settings`). The list of programs only shows what the station reports.
 * - Disconnect is the station's (`tci_server_disconnect` is refused from afar) and Copy hands over
 *   an address on the station's computer, so from afar neither is drawn.
 */
import type { ReactNode } from 'react'
import { t } from '../i18n'
import { T } from '../i18n/T'
import type { Settings, TciProgramStatus, TciServerStatus } from '../types'
import { parsePort } from './Js8ApiSettings'

export type TciServerSwitch = 'tciServer' | 'tciServerTx' | 'tciServerCw' | 'tciServerSpots'
export type TciServerKey = TciServerSwitch | 'tciServerPort'

/** The port the server comes on: Rust's `TCI_SERVER_DEFAULT_PORT` (settings.rs), where WSJT-X,
 *  JTDX and MSHV look first. `TciServerSettings.test.tsx` reads settings.rs and fails if the two
 *  drift. */
export const TCI_SERVER_DEFAULT_PORT = 40001

/** The port the CAT broker and the TCI server would both ask for, while both are on and on one
 *  port: whichever starts second cannot have it. `null` when either is off or the ports differ. */
export function sharePortClash(s: Partial<Settings> | null | undefined): number | null {
  if (!s?.catBroker || !s.tciServer) return null
  const tci = s.tciServerPort ?? TCI_SERVER_DEFAULT_PORT
  return s.catBrokerPort === tci ? tci : null
}

export interface TciServerSettingsProps {
  form: Settings
  /** The server as the station runs it (`radio.tciServer`): absent while it is off. */
  status?: TciServerStatus | null
  onSwitch: (key: TciServerSwitch, on: boolean) => void
  onPort: (port: number) => void
  /** The operator's Disconnect for one program, by its number. */
  onDisconnect: (id: number) => void
  /** True from a computer driving the station, where each of these is the station's to set. */
  locked: (key: TciServerKey) => boolean
  /** "Set at the station" for a setting that is, from afar; nothing at the station. */
  setAtStation: (key: TciServerKey) => ReactNode
}

export function TciServerSettings({
  form,
  status,
  onSwitch,
  onPort,
  onDisconnect,
  locked,
  setAtStation,
}: TciServerSettingsProps) {
  const on = form.tciServer === true
  const atStation = !locked('tciServer')
  const address = on && status?.port != null && !status.error ? `127.0.0.1:${status.port}` : null
  const programs = status?.programs ?? []
  const label = t('settings.transmit.tciServer.label')

  // A switch on its own row: the CW and the spots tiers, each on as it comes.
  const tier = (key: 'tciServerCw' | 'tciServerSpots', name: string, hint: string) => {
    const value = form[key] !== false
    return (
      <div className="settings-field">
        <label className="settings-toggle">
          <span className="settings-label">{name}</span>
          <button disabled={locked(key)} data-setting={key}
            type="button"
            role="switch"
            aria-checked={value}
            aria-label={name}
            className={`toggle${value ? ' on' : ''}`}
            onClick={() => onSwitch(key, !value)}
          >
            <span className="toggle-knob" />
          </button>
        </label>
        {setAtStation(key)}
        <span className="settings-hint">{hint}</span>
      </div>
    )
  }

  const program = (p: TciProgramStatus) => {
    const doing = [
      p.audio ? t('settings.transmit.tciServer.program.audio') : null,
      p.keyed ? t('settings.transmit.tciServer.program.keyed') : null,
    ].filter((s): s is string => s !== null)
    // A field of its own, so its lines stack: the row with its address and Disconnect, what it is
    // doing, the last refusal.
    return (
      <div role="listitem" className="settings-field" key={p.id}>
        <div className="rig-share-row">
          <span>{t('settings.transmit.tciServer.program.name', { id: p.id })}</span>
          {/* Where it connects from: an address, shown as the station gives it. */}
          <code className="rig-share-addr mono">{p.peer}</code>
          {atStation && (
            <button
              type="button"
              className="settings-linkbtn"
              onClick={() => onDisconnect(p.id)}
              title={t('settings.transmit.tciServer.disconnect.title')}
            >
              {t('settings.transmit.tciServer.disconnect.action')}
            </button>
          )}
        </div>
        {doing.length > 0 && <span className="settings-hint">{doing.join(' · ')}</span>}
        {p.refused && (
          <span className="settings-hint" data-tci-program-refused>
            {t('settings.transmit.tciServer.program.refused', { why: p.refused })}
          </span>
        )}
      </div>
    )
  }

  return (
    <div className="settings-field" data-tci-server>
      <span className="settings-label">{label}</span>
      <div className="rig-share-row">
        <button disabled={locked('tciServer')} data-setting="tciServer"
          type="button"
          role="switch"
          aria-checked={on}
          aria-label={label}
          className={`toggle${on ? ' on' : ''}`}
          onClick={() => onSwitch('tciServer', !on)}
        >
          <span className="toggle-knob" />
        </button>
        {setAtStation('tciServer')}
        {/* With the bind refused the address is not a fact: the station's sentence takes its
            place, and with it the way out (another port). */}
        {on && status?.error && (
          <span className="rig-share-fault" role="alert">
            {status.error}
          </span>
        )}
        {address && (
          <>
            <code className="rig-share-addr mono" data-tci-server-address>
              {address}
            </code>
            {atStation && (
              <button
                type="button"
                className="settings-linkbtn"
                onClick={() => {
                  void navigator.clipboard?.writeText(address).catch(() => {})
                }}
                title={t('settings.transmit.share.copy.title')}
              >
                {t('settings.transmit.share.copy.action')}
              </button>
            )}
          </>
        )}
      </div>
      {on && !status && (
        <span className="settings-hint" role="status" data-tci-server-waiting>
          {t('settings.transmit.tciServer.waiting')}
        </span>
      )}
      <span className="settings-hint">
        {on ? (
          <T k="settings.transmit.tciServer.hint.on" tags={{ em: <em /> }} />
        ) : (
          <T k="settings.transmit.tciServer.hint.off" tags={{ b: <strong /> }} />
        )}
      </span>
      {on && (
        <>
          <label className="settings-field">
            <span className="settings-label">{t('settings.transmit.tciServer.port.label')}</span>
            <input disabled={locked('tciServerPort')} data-setting="tciServerPort"
              className="settings-input"
              type="number"
              inputMode="numeric"
              min={1}
              max={65535}
              value={String(form.tciServerPort ?? TCI_SERVER_DEFAULT_PORT)}
              placeholder={String(TCI_SERVER_DEFAULT_PORT)}
              onChange={(e) => {
                const port = parsePort(e.target.value)
                if (port !== null) onPort(port)
              }}
              autoComplete="off"
            />
            {setAtStation('tciServerPort')}
            <span className="settings-hint">{t('settings.transmit.tciServer.port.hint')}</span>
          </label>
          {/* ⛔ THE TRANSMIT SWITCH: drawn at the station only (see the header). From afar the row
              keeps its name and says where it is set, with no switch, not even a read-only one. */}
          <div className="settings-field" data-tci-server-tx>
            <label className="settings-toggle">
              <span className="settings-label">{t('settings.transmit.tciServer.tx.label')}</span>
              {!locked('tciServerTx') && (
                <button disabled={locked('tciServerTx')} data-setting="tciServerTx"
                  type="button"
                  role="switch"
                  aria-checked={form.tciServerTx === true}
                  aria-label={t('settings.transmit.tciServer.tx.label')}
                  className={`toggle${form.tciServerTx === true ? ' on' : ''}`}
                  onClick={() => onSwitch('tciServerTx', form.tciServerTx !== true)}
                >
                  <span className="toggle-knob" />
                </button>
              )}
            </label>
            {locked('tciServerTx') && (
              <span className="settings-hint" data-set-at-station="tciServerTx">
                {t('settings.stationOnly.hint')}
              </span>
            )}
            <span className="settings-hint">{t('settings.transmit.tciServer.tx.hint')}</span>
            <span className="settings-hint" data-tci-server-slot-timed>
              {t('settings.transmit.tciServer.tx.slotTimed')}
            </span>
          </div>
          {tier(
            'tciServerCw',
            t('settings.transmit.tciServer.cw.label'),
            t('settings.transmit.tciServer.cw.hint'),
          )}
          {tier(
            'tciServerSpots',
            t('settings.transmit.tciServer.spots.label'),
            t('settings.transmit.tciServer.spots.hint'),
          )}
          <div className="settings-field" data-tci-server-programs>
            <span className="settings-label">{t('settings.transmit.tciServer.programs.label')}</span>
            {programs.length === 0 ? (
              <span className="settings-hint">{t('settings.transmit.tciServer.programs.none')}</span>
            ) : (
              <div role="list" className="settings-field">
                {programs.map(program)}
              </div>
            )}
          </div>
        </>
      )}
    </div>
  )
}
