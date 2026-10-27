// The TCI connection as the screens offer it: Settings ▸ Radio ▸ Rig & CAT's "TCI" choice and the
// setup wizard's. TCI is an SDR program's own server (ExpertSDR3, Thetis, piHPSDR): one WebSocket
// that carries the program's CAT, and in time its audio, its spectrum and its spots.
//
// ⚠️ THIS FILE IS ON THE MIGRATED LIST (i18n/hardcoded-strings.test.ts). What stays here unlocalised
// is the vocabulary of other programs and of the wire: the programs' and radios' own names, the
// protocol's name, addresses and port numbers. Every sentence comes from the catalog.
import { t } from './i18n'
import type { TciProbeResult } from './api'

/** The rig model a TCI radio stores: Hamlib's own number for TCI, so the radio reads
 *  "TCI" where a model is shown. Nexus's TCI client serves it, and nothing else may: the rigctld
 *  Nexus ships cannot load 7 (`rigmodels.rs`), which is why leaving the TCI connection takes the
 *  number back off. */
export const TCI_RIG_MODEL = 7

/** The model name a TCI radio shows: the protocol's own name, never translated. */
export const TCI_RIG_MODEL_NAME = 'TCI'

/** The SDR program on this computer, the usual station: Rust's `TCI_DEFAULT_HOST`. */
export const TCI_DEFAULT_HOST = '127.0.0.1'

/**
 * The dialect choices as `tci_dialect` stores them, with the port each program's TCI server comes
 * set to and the names the choice shows (tokens: programs and radios). `auto` leaves it to Nexus,
 * which learns the program from what it does.
 *
 * ⚠️ MIRRORS `TciDialect::as_setting` and `TciDialect::default_port` in
 * crates/tempo-app/src/settings.rs, and `tci.test.ts` reads that file and fails if the two drift:
 * Find TCI server tries Rust's ports, and this list pre-fills the same ones.
 */
export const TCI_DIALECTS: readonly { value: string; port: number; label: string | null }[] = [
  { value: 'auto', port: 50001, label: null },
  { value: 'expertsdr3', port: 50001, label: 'ExpertSDR3 (SunSDR, MB1, Colibri)' },
  { value: 'thetis', port: 50001, label: 'Thetis (ANAN, Hermes Lite 2)' },
  { value: 'pihpsdr', port: 40001, label: 'piHPSDR' },
]

/** The port a new TCI radio starts on: `auto`'s, Rust's `TCI_DEFAULT_PORT`. */
export const TCI_DEFAULT_PORT = TCI_DIALECTS[0].port

/** How many transceivers the Transceiver choice offers: AetherSDR serves one per Flex slice, eight
 *  at most; ExpertSDR3 serves two and piHPSDR one. */
export const TCI_TRX_CHOICES = 8

/** The transceivers to offer, TCI's numbers from 0 (shown from 1): the usual eight, and a stored
 *  number past them too, so the choice always shows what the radio holds. */
export function tciTrxChoices(stored: number): number[] {
  return Array.from({ length: Math.max(TCI_TRX_CHOICES, stored + 1) }, (_, i) => i)
}

/** The dialect choice a stored `tci_dialect` shows, read as Rust reads it
 *  (`TciDialect::from_setting`): case and surrounding space do not matter, and any other word (a
 *  newer build's, a hand edit) is `auto`, never a program the operator did not name. */
export function tciDialectShown(stored: string | undefined): string {
  const word = (stored ?? '').trim().toLowerCase()
  return TCI_DIALECTS.some((d) => d.value === word) ? word : 'auto'
}

/**
 * The port to show once the operator names a dialect. Naming a program fills in the port it comes
 * set to, but only over another program's port (or none): a port the operator typed is theirs, and
 * leaving it to Nexus (`auto`) never moves the port, because it says nothing about where the
 * program listens.
 */
export function tciPortForDialect(port: number, dialect: string): number {
  const named = TCI_DIALECTS.find((d) => d.value === dialect && d.value !== 'auto')
  if (!named) return port
  const someDefault = port === 0 || TCI_DIALECTS.some((d) => d.port === port)
  return someDefault ? named.port : port
}

/** What a found server said about itself, as one token: its program and version, then its radio
 *  (`ExpertSDR3 2.0, SunSDR2DX`). Both are the server's own words, shown as sent. */
export function tciServerName(found: NonNullable<TciProbeResult['found']>): string {
  const program = [found.program, found.version].filter((s): s is string => !!s).join(' ')
  const named = [program, found.device ?? ''].filter((s) => s !== '').join(', ')
  return named || t('settings.rigControl.tci.find.unnamed')
}

/**
 * The status line after Find TCI server: what answered where, in the order the ports were tried,
 * and, when the found port is not the one the form holds, that it has been filled in (`portSet`).
 */
export function tciProbeMessage(r: TciProbeResult, portSet = false): string {
  if (!r.resolved) return t('settings.rigControl.tci.find.badHost', { host: r.host })
  const said: string[] = []
  for (const { port, outcome } of r.tried) {
    if (outcome === 'nexus') said.push(t('settings.rigControl.tci.find.nexus', { port }))
    else if (outcome === 'notTci') said.push(t('settings.rigControl.tci.find.notTci', { port }))
    else if (outcome === 'noReady') said.push(t('settings.rigControl.tci.find.noReady', { port }))
  }
  const found = r.found
  if (!found) {
    const ports = r.tried.map((p) => String(p.port)).join(', ')
    return [...said, t('settings.rigControl.tci.find.none', { host: r.host, ports })].join(' ')
  }
  const out = [t('settings.rigControl.tci.find.found', { address: `${r.host}:${found.port}`, server: tciServerName(found) })]
  if (found.trxCount != null) out.push(t('settings.rigControl.tci.find.trxCount', { count: found.trxCount }))
  if (found.dialect === 'unknown') out.push(t('settings.rigControl.tci.find.unknownProgram'))
  if (portSet) out.push(t('settings.rigControl.tci.find.portSet', { port: found.port }))
  return [...out, ...said].join(' ')
}
