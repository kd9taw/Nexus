// The operator's geographic ALERT scope (#174), resolved for the decode alert path.
//
// The hook is the only stateful piece: it reads the two Settings fields, fetches the cty.dat
// continent table when — and only when — a continent is actually ticked, and hands `alerts.ts` a
// flat set of entity NAMES. The rules themselves live in `dxccGeo.ts`, pure and shared with every
// other surface that filters by where a station is.
//
// ⚠️ THIS IS NOT `countryExclude`, and the distinction is load-bearing. That filter answers
// "don't SHOW me these rows" and exempts anything NEEDED (`OVERRIDING_NEEDS`) — an operator hides
// the United States to thin a busy waterfall, not to lose their ATNO alert, and its own header
// states it never reaches the alerts. This one answers "don't INTERRUPT me about these places",
// which is a different intent with a different exemption list (someone calling you, and a
// watch-list hit). They share a vocabulary, deliberately, and nothing else.

import { useEffect, useMemo, useState } from 'react'
import { dxccContinentTable } from './countryExclude'
import { scopedEntities } from './dxccGeo'

/**
 * The entity names the operator's alert scope admits, or `null` for "no scope — alert on
 * everything", which is what an operator who has never opened the setting gets.
 *
 * The table is fetched lazily: an unconfigured operator, and one who scoped by entity NAME only,
 * costs no IPC at all. Until it arrives a continent scope resolves to `null` rather than to an
 * empty set — `scopedEntities` owns that rule, and it is why a slow table delays nothing and
 * silences nothing.
 */
export function useAlertGeoScope(
  continents: string[] | undefined,
  entities: string[] | undefined,
): ReadonlySet<string> | null {
  const [table, setTable] = useState<ReadonlyMap<string, string> | null>(null)
  const needsTable = (continents?.length ?? 0) > 0

  useEffect(() => {
    if (!needsTable || table) return
    let live = true
    void dxccContinentTable().then((m) => {
      if (live && m.size > 0) setTable(m)
    })
    return () => {
      live = false
    }
  }, [needsTable, table])

  // Keyed on the CONTENTS, not the array identity: Settings hands back a fresh array on every
  // load, and a scope that re-resolved on each one would rebuild the set every poll.
  const contKey = (continents ?? []).join(',')
  const entKey = (entities ?? []).join(',')
  return useMemo(
    () => scopedEntities(contKey ? contKey.split(',') : [], entKey ? entKey.split(',') : [], table),
    [contKey, entKey, table],
  )
}
