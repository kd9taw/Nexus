// @vitest-environment jsdom
//
// ONE QRZ PAGE PER STATION, FROM EVERY BUTTON. The report: an APRS iGate's card, PI1UTR-10, and
// its QRZ button opened https://www.qrz.com/db/PI1UTR10, which is somebody else's callsign. The
// places that build a QRZ link each had their own copy of the rule, and the copies disagreed with
// each other on ties and on text outside ASCII.
//
// `__fixtures__/qrz-urls.json` is ONE table of what a callsign opens. This file holds the
// DXpedition link to it, row by row, and `qrz_url_is_the_shared_tables` in src-tauri holds the
// desktop's `qrz_url` to the same rows. Change a row and both must agree again.
import { describe, expect, it } from 'vitest'
import { readFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { dxpedLink } from './components/prop/dxpedLink'

interface Case {
  call: string
  url: string | null
  why: string
}
// A path, not `new URL(…, import.meta.url)`: under jsdom that URL is not a file URL Node can read.
const TABLE = JSON.parse(readFileSync(resolve(dirname(fileURLToPath(import.meta.url)), '__fixtures__/qrz-urls.json'), 'utf8')) as {
  cases: Case[]
}

describe('the QRZ page a callsign opens', () => {
  it('the table only ever blesses a QRZ page of letters and digits', () => {
    expect(TABLE.cases.length).toBeGreaterThan(0)
    for (const { call, url } of TABLE.cases) {
      if (url !== null) expect(url, call).toMatch(/^https:\/\/www\.qrz\.com\/db\/[A-Z0-9]+$/)
    }
  })

  it.each(TABLE.cases)('the DXpedition link for "$call" ($why)', ({ call, url }) => {
    expect(dxpedLink({ call })).toEqual(url === null ? null : { kind: 'qrz', url, label: 'QRZ' })
  })
})
