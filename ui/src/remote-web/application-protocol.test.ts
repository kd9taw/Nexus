import { expect, it } from 'vitest'
import { applicationRequest, applicationReply, applyApplicationReply, APPLICATION_COMMANDS } from './application-protocol'

const requestId = '8aa041cb-c642-459c-83f3-11a5b720647d'
it('admits only the reviewed application reads with bounded version cursors', () => {
  for (const command of APPLICATION_COMMANDS) {
    expect(applicationRequest({ type: 'applicationRead', requestId, command, revision: null }).command).toBe(command)
  }
  for (const change of [{ command: 'halt_tx' }, { command: 'get_credentials_status' }, { args: {} },
    { revision: -1 }, { revision: Number.MAX_SAFE_INTEGER + 1 }, { requestId: '../other-station' }]) {
    expect(() => applicationRequest({ type: 'applicationRead', requestId, command: 'get_snapshot', revision: null, ...change })).toThrow()
  }
})

it('applies a delta only to its exact acknowledged base and never revives missing state', () => {
  const full = applicationReply({ type: 'applicationResult', requestId, command: 'get_snapshot', revision: 1,
    baseRevision: null, ageMs: 0, data: { mycall: 'TEST', radio: { dialMhz: 14.074 }, activePeer: 'TEST2' }, removed: [] })
  const first = applyApplicationReply(null, full)
  const delta = applicationReply({ ...full, revision: 2, baseRevision: 1, data: { radio: { dialMhz: 7.074 } }, removed: ['activePeer'] })
  expect(applyApplicationReply(first, delta)).toEqual({ revision: 2, value: { mycall: 'TEST', radio: { dialMhz: 7.074 } } })
  expect(() => applyApplicationReply(null, delta)).toThrow()
  expect(() => applyApplicationReply({ ...first, revision: 3 }, delta)).toThrow()
  expect(first.value).toEqual({ mycall: 'TEST', radio: { dialMhz: 14.074 }, activePeer: 'TEST2' })
})

it('rejects unknown fields, prototype keys, unsafe ages and mixed success/error results', () => {
  const full = { type: 'applicationResult', requestId, command: 'get_settings', revision: 1,
    baseRevision: null, ageMs: 0, data: { units: 'imperial' }, removed: [] }
  expect(applicationReply(full).command).toBe('get_settings')
  for (const change of [{ ageMs: -1 }, { ageMs: 3000 }, { error: 'unavailable' }, { revision: 0 },
    { data: JSON.parse('{"__proto__":{}}') }, { removed: ['constructor'] }, { extra: true }]) {
    expect(() => applicationReply({ ...full, ...change })).toThrow()
  }
})
