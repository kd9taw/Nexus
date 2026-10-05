// Remote over this network, on the computer the operator works FROM: the sentence for why the
// station could not be reached, or found by name. The codes are `tempo_stream::lan::unreached`'s.
//
// WINDOWS' OWN PROMPT (the operator's ruling of 2026-10-04): the station adds no firewall rule of
// its own, and from here a firewall that drops what it does not allow, a port nothing listens on
// behind that firewall, a network that keeps its devices apart and a station that is asleep all
// look the same. So the no-answer sentence names each, says it cannot tell which, and never
// blames one. Typing the address always works, found by name or not.
import { t } from '../i18n'

/** Why the station could not be reached (`tempo_stream::lan::unreached`): what this computer's
 *  Nexus says when nothing answered, as against a station that answered and said no. */
export const LAN_UNREACHED = ['otherNetwork', 'refused', 'noAnswer'] as const
export type LanUnreached = (typeof LAN_UNREACHED)[number]

export function lanUnreached(reason: string): reason is LanUnreached {
  return (LAN_UNREACHED as readonly string[]).includes(reason)
}

export function lanReachLine(code: LanUnreached): string {
  switch (code) {
    case 'otherNetwork': return t('remote.lan.reach.otherNetwork')
    case 'refused': return t('remote.lan.reach.refused')
    case 'noAnswer': return t('remote.lan.reach.noAnswer')
  }
}

/** No station found by name: Windows' name service cannot be used here (`available` false), or
 *  nothing answered. Either way, the address the station shows is typed. */
export function lanFindLine(available: boolean): string {
  return available ? t('remote.lan.find.none') : t('remote.lan.find.unavailable')
}

/** The examples a mistyped address is shown: an address as the station shows one, and with its
 *  port. Invariant tokens, kept out of the catalog's prose. */
const EXAMPLE = '192.168.1.20'
const EXAMPLE_PORT = 42075

/** A typed address that is not one the station could show (`tempo_stream::lan::typed`). */
export function lanTypedLine(): string {
  return t('remote.lan.typed.invalid', { address: EXAMPLE, withPort: `${EXAMPLE}:${EXAMPLE_PORT}` })
}
