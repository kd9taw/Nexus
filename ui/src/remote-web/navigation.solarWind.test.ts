// The hosted page reads the station's Connect document; the solar-wind sample in it changed shape.
//
// A station of this build sends speed and density as null when they are not known, and dates the
// sample (`timeUnix`). An older station sends 0 and no time. The page must accept both, from either
// side of a deploy, and hand the sample through untouched: its display code is where "not known" and
// "how old" are decided, never the validator. This file also runs, unchanged, against the page as it
// is deployed, which is how the report proves the deployed page accepts a newer station.
import { describe, expect, it, vi } from 'vitest'
import connect from './__fixtures__/navigation-connect.json'
import { loadNavigation } from './navigation'
import type { RemoteCollections } from './collections'
import type { QueryPage } from './application-query-protocol'
import { navigationPages } from './__fixtures__/navigation-page'

function source(pages: QueryPage[]): RemoteCollections {
  return {
    page: vi.fn(async () => {
      const page = pages.shift()
      if (!page) throw new Error('unexpectedPage')
      return structuredClone(page)
    }),
  } as unknown as RemoteCollections
}

/** The Connect fixture with `solarWind` set on its snapshot. */
function withWind(solarWind: Record<string, unknown>) {
  return { ...connect, prop: { ...connect.prop, spaceWx: { ...connect.prop.spaceWx, solarWind } } }
}

describe("the hosted page reads a station's solar-wind sample", () => {
  it('from a station of this build: not known is null, and the sample is dated', async () => {
    const doc = withWind({ bzNt: -3.4, btNt: null, speedKms: null, density: null, timeUnix: connect.prop.asOf - 120 })
    const result = await loadNavigation<typeof doc>(source(navigationPages('connect', doc)), 'connect', '', () => true)
    expect(result.value.prop.spaceWx.solarWind).toEqual(doc.prop.spaceWx.solarWind)
  })

  it('from an older station: 0 for not known, and no time', async () => {
    const doc = withWind({ bzNt: -3.4, btNt: 6.1, speedKms: 0, density: 0 })
    const result = await loadNavigation<typeof doc>(source(navigationPages('connect', doc)), 'connect', '', () => true)
    expect(result.value.prop.spaceWx.solarWind).toEqual(doc.prop.spaceWx.solarWind)
  })

  it('control: the validator does read the space weather the sample sits in', async () => {
    const doc = { ...connect, prop: { ...connect.prop, spaceWx: [] } }
    await expect(loadNavigation(source(navigationPages('connect', doc)), 'connect', '', () => true)).rejects.toThrow()
  })
})
