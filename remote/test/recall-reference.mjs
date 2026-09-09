// Execute the existing UI's pure history functions as the independent native
// parity oracle. TypeScript is already an explicit UI build dependency. The
// only loaded source modules are local band.ts and features/callHistory.ts.
import { readFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
const require = createRequire(new URL('../../ui/package.json', import.meta.url))
const ts = require('typescript')
export async function recallReference() {
  async function module(path, dependencies = {}) {
    const source = await readFile(new URL(`../../ui/src/${path}`, import.meta.url), 'utf8')
    const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 } })
    const exports = {}
    new Function('exports', 'require', outputText)(exports, name => {
      if (!(name in dependencies)) throw new Error(`Unexpected recall reference dependency: ${name}`)
      return dependencies[name]
    })
    return exports
  }
  const band = await module('band.ts')
  return { ...band, ...await module('features/callHistory.ts', { '../band': band }) }
}

export function recallAdif() {
  const rows = [
    ['W1AW', '20M', 'CW', 14.025], ['W1AW', '40m', 'FT8', 7.074], ['W1AW/P', '80m', 'SSB', 3.8],
    ['DL1ABC', 'odd-import', 'LSB', 0], ['DL1ABC', 'odd-import', 'USB', 7.142], ['DL1ABC', '', 'FT4', 0],
    ...[1.8, 2, 3.5, 4, 5.3, 5.41, 5.42, 7, 7.3, 10.1, 10.15, 14, 14.35, 18.06, 18.068, 18.168, 18.17,
      21, 21.45, 24.89, 24.99, 28, 29.7, 50, 54, 70, 71, 144, 148, 222, 225, 420, 450, 902, 928,
      1240, 1300, 2300, 2450, 3300, 3500, 5650, 5925, 10000, 10500, 24000, 24250]
      .map((f, i) => ['JA1ABC', 'legacy-band', ['FT8', 'FT4', 'BPSK31', 'PSK31', 'BPSK63', 'FM', 'AM'][i % 7], f]),
  ]
  return rows.map(([call, band, mode, freq], i) => {
    const fields = { CALL: call, BAND: band, MODE: mode, FREQ: String(freq), COUNTRY: call.startsWith('DL') ? 'Germany' : '',
      QSO_DATE: '20260909', TIME_ON: `01${String(i).padStart(2, '0')}00`, QSL_RCVD: i % 3 ? 'N' : 'Y', NOTES: i === 0 ? 'Recall parity note' : '' }
    return Object.entries(fields).map(([k, v]) => `<${k}:${v.length}>${v}`).join('') + '<EOR>\n'
  }).join('')
}
