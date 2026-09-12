// Execute the shipped UI roll-up and Remote parser as independent comparators
// for the native summary. This synthetic log never reaches a deployed bundle.
import { readFile } from 'node:fs/promises'
import { createRequire } from 'node:module'
const require = createRequire(new URL('../../ui/package.json', import.meta.url))
const ts = require('typescript')
export async function insightsReference() {
  async function module(path, dependencies = {}) {
    const source = await readFile(new URL(`../../ui/src/${path}`, import.meta.url), 'utf8')
    const { outputText } = ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 } })
    const exports = {}
    new Function('exports', 'require', outputText)(exports, name => {
      if (!(name in dependencies)) throw new Error(`Unexpected insights reference dependency: ${name}`)
      return dependencies[name]
    })
    return exports
  }
  const protocol=await module('remote-web/application-protocol.ts')
  const stream=await module('remote-web/application-stream-protocol.ts',{'./application-protocol':protocol})
  const query=await module('remote-web/application-query-protocol.ts',{'./application-protocol':protocol,'./application-stream-protocol':stream})
  const display=await module('remote-web/display-validation.ts')
  const statistics = await module('features/logStats.ts')
  return { ...statistics, ...await module('remote-web/navigation.ts',{'./application-query-protocol':query,'./configuration':await module('remote-web/configuration.ts',{'./configuration-schema':await module('remote-web/configuration-schema.ts'),'./display-validation':display}),'./display-validation':display,'./application-stream-protocol':stream,'../features/satVfo':await module('features/satVfo.ts',{'../i18n':{t:key=>key}})}), ...await module('remote-web/sstv.ts',{'./application-query-protocol':query,'./display-validation':display}),
    ...await module('remote-web/aprs.ts',{'./display-validation':display}), ...await module('remote-web/insights.ts', { '../features/logStats': statistics }),
    ...await module('remote-web/dxpeditions.ts'),
    ...await module('remote-web/ota.ts'),
    ...await module('remote-web/field-day.ts'),
    ...await module('remote-web/js8.ts', { './application-protocol': await module('remote-web/application-protocol.ts') }),
    ...await module('remote-web/memories.ts', { '../remote-native/memoryBank': await module('remote-native/memoryBank.ts') }) }
}
export function insightsAdif() {
  const rows = [
    { CALL: 'W1AW', COUNTRY: 'United States', STATE: 'ct', QSL_RCVD: 'Y', CREDIT_GRANTED: 'DXCC', IOTA: 'NA-001' },
    { CALL: 'K6ABC', COUNTRY: 'united states', STATE: 'CA', LOTW_QSL_RCVD: 'Y', IOTA: 'NA-001' },
    { CALL: 'VK6ABC', COUNTRY: 'Australia', STATE: 'WA', EQSL_QSL_RCVD: 'Y' },
    { CALL: 'DL1ABC', COUNTRY: 'Germany', LOTW_QSL_RCVD: 'Y' },
    { CALL: 'DL2ABC', COUNTRY: 'Fed. Rep. of Germany', QSL_RCVD: 'Y' },
    { CALL: 'K1SAT', BAND: '2m', GRIDSQUARE: 'FN31', PROP_MODE: 'SAT', SAT_NAME: 'AO-91', LOTW_QSL_RCVD: 'Y' },
    { CALL: '000', COUNTRY: 'Åland', TIME_ON: '000000' },
    { CALL: '001', COUNTRY: 'aland', TIME_ON: '000001' },
    ...Array.from({ length: 2010 }, (_, i) => ({ CALL: `K1S${i}`, BAND: i % 2 ? '20m' : '40m', MODE: i % 3 ? 'FT8' : 'CW', QSO_DATE: i % 2 ? '20260909' : '20240229' })),
  ]
  return rows.map((row, i) => Object.entries({ BAND: '20m', MODE: 'FT8', QSO_DATE: '20260909', TIME_ON: '123456',
    NOTES: `synthetic note ${i}`, ...row }).map(([key, value]) => `<${key}:${Buffer.byteLength(value)}>${value}`).join('') + '<EOR>\n').join('')
}
