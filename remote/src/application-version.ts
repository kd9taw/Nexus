// The application wire ladder, declared ONCE. Both numbers this service puts on the wire - the
// version it advertises in /config and the version it negotiates with a connecting station - are
// derived from the table below, and so is the check that a version handed back to a room is one
// this build knows. The five hand-written spellings these replaced (a bare `17`, a fifteen-deep
// nested ternary, and three `[1..17]` array literals) had no mechanism keeping them equal.
//
// HOW A STATION ADVERTISES. Capabilities are cumulative, so a station's version is the longest
// unbroken PREFIX of this table that it sent: entry i (0-based) at its pinned value, with every
// entry above it also at its pinned value, means version i + 2. A station that sends none of them
// falls back to the legacy header, which is the only one whose value is read as a number. The
// prefix rule is why a missing header costs every capability above it as well as its own, and why
// a new capability is a new ROW AT THE BOTTOM - never a bump of an existing row's value.
//
// EXTENSION-HEADER AUDIT (2026-09-16). Every row below is PINNED: the service compares the header
// against one exact string, and anything else - a different digit, a malformed value, no header -
// reads as absent and stops the prefix there. So a pinned header carries exactly one bit (sent /
// not sent) and the digit it sends is decoration: the rung comes from the row's POSITION, not from
// the number. All seventeen earn their place - each is the only signal for its rung, and an older
// station that stops at any rung still negotiates that rung exactly - but only the first accepts a
// value other than the one the current desktop sends.
//
//   ver  header                                      sent  other values accepted
//     1  x-nexus-application-version                 1     YES - '1' or '2', read as the version
//     2  x-nexus-application-stream-version          2     no - '2' only (live instrument stream)
//     3  x-nexus-application-query-version           1     no - '1' only (snapshot/delta queries)
//     4  x-nexus-application-recall-version          1     no - '1' only
//     5  x-nexus-application-keyboard-version        1     no - '1' only
//     6  x-nexus-application-insights-version        1     no - '1' only
//     7  x-nexus-application-dxpeditions-version     1     no - '1' only
//     8  x-nexus-application-memories-version        1     no - '1' only
//     9  x-nexus-application-ota-version             1     no - '1' only
//    10  x-nexus-application-field-day-version       1     no - '1' only
//    11  x-nexus-application-js8-version             1     no - '1' only
//    12  x-nexus-application-station-modes-version   1     no - '1' only (SSTV + APRS)
//    13  x-nexus-application-navigation-version      1     no - '1' only (satellites)
//    14  x-nexus-application-configuration-version   1     no - '1' only
//    15  x-nexus-application-lookups-version         1     no - '1' only (parks, confirmations)
//    16  x-nexus-application-alerts-version          1     no - '1' only (Pounce)
//    17  x-nexus-application-rotator-version         1     no - '1' only
//
// Version 1 has no row: it is the legacy `x-nexus-application-version` header itself, which
// predates the ladder and is the one place a station's own number is believed ('2' there is an
// old desktop that advertised the stream capability before it had its own header). Removing a row
// is a wire-compatibility decision, not a cleanup: a station still sending the header it names
// must keep negotiating the rung it names.
export const APPLICATION_EXTENSIONS: readonly (readonly [header: string, pinned: string])[] = [
  ['x-nexus-application-stream-version', '2'],
  ['x-nexus-application-query-version', '1'],
  ['x-nexus-application-recall-version', '1'],
  ['x-nexus-application-keyboard-version', '1'],
  ['x-nexus-application-insights-version', '1'],
  ['x-nexus-application-dxpeditions-version', '1'],
  ['x-nexus-application-memories-version', '1'],
  ['x-nexus-application-ota-version', '1'],
  ['x-nexus-application-field-day-version', '1'],
  ['x-nexus-application-js8-version', '1'],
  ['x-nexus-application-station-modes-version', '1'],
  ['x-nexus-application-navigation-version', '1'],
  ['x-nexus-application-configuration-version', '1'],
  ['x-nexus-application-lookups-version', '1'],
  ['x-nexus-application-alerts-version', '1'],
  ['x-nexus-application-rotator-version', '1'],
]

/** The newest application version this build speaks: the legacy base plus every rung above it. */
export const APPLICATION_VERSION = APPLICATION_EXTENSIONS.length + 1

/** What a connecting station advertised, from its request headers. 0 means no application
 *  protocol at all - the station gets frames and nothing else. */
export function negotiatedApplicationVersion(headers: Headers): number {
  const legacy = headers.get('x-nexus-application-version') ?? ''
  let version = legacy === '1' || legacy === '2' ? Number(legacy) : 0
  for (let rung = 0; rung < APPLICATION_EXTENSIONS.length; rung++) {
    const [header, pinned] = APPLICATION_EXTENSIONS[rung]
    if (headers.get(header) !== pinned) break
    version = rung + 2
  }
  return version
}

/** Whether a version carried back from an admission or a hibernated attachment is one this build
 *  negotiated. Anything else - a rolled-back deploy's higher number, a field that never existed,
 *  a non-integer - is not a version here and is treated as 0. */
export function knownApplicationVersion(version: number): boolean {
  return Number.isInteger(version) && version >= 1 && version <= APPLICATION_VERSION
}
