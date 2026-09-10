import { describe, expect, it } from 'vitest'
import fixture from './__fixtures__/js8.json'
import { js8DisplayNow, parseJs8Context, parseJs8Sample } from './js8'
import type { QueryPage } from './application-query-protocol'

describe('JS8 observation values', () => {
  it('keeps every native display field while accounting for clock difference and transport age', () => {
    const current = structuredClone(fixture)
    const state = parseJs8Sample(current,250,current.capturedAtMs+60_000)
    expect(state).toEqual(current.state)
    const browserNow = current.capturedAtMs + 60_000
    expect(js8DisplayNow(state, browserNow)).toBe(current.capturedAtMs + 250)
    expect(js8DisplayNow(state, browserNow + 1000)).toBe(current.capturedAtMs + 1250)
    expect(js8DisplayNow(state, browserNow - 1000)).toBe(current.capturedAtMs + 250)
    // WAN age/clock jitter must not change native activity row identities.
    const later = parseJs8Sample({...current,capturedAtMs:current.capturedAtMs+500},350,browserNow+600)
    expect(later.activity.map(r => r.atMs)).toEqual(state.activity.map(r => r.atMs))
    expect(js8DisplayNow(later,browserNow+600)).toBe(current.capturedAtMs+850)
    expect(state.inbox[0].text).toBe('STORED REMOTE TEST')
    expect(state.armed).toEqual(current.state.armed)
    expect(current).toEqual(fixture)
  })
  it('rejects expired, malformed, oversized and contradictory armed state', () => {
    expect(() => parseJs8Sample(fixture,3000)).toThrow()
    for (const change of [
      (s: typeof fixture.state) => { s.armed.autoreply=true },
      (s: typeof fixture.state) => { s.activity[0].text='x'.repeat(1025) },
      (s: typeof fixture.state) => { s.inbox.push({...s.inbox[0]}) },
      (s: typeof fixture.state) => { s.queue=Array.from({length:2049},()=>s.queue[0]) },
      (s: typeof fixture.state) => { s.stations[0].freqHz=NaN },
    ]) {
      const value=structuredClone(fixture);change(value.state)
      expect(() => parseJs8Sample(value,0)).toThrow()
    }
    expect(() => parseJs8Sample({...fixture,unreviewed:true},0)).toThrow()
  })
  it('distinguishes an explicit never-worked answer from absent or expired history', () => {
    const page = {collection:'js8Context',offset:0,total:0,retained:0,rows:[],nextCursor:null,
      meta:{capturedAgeMs:0,source:{plan:[],history:{W1AW:{count:2,lastUnix:1700000000,grid:'',name:'',comment:''},
        K2ABC:{count:0,lastUnix:null,grid:'',name:'',comment:''}}}}} as unknown as QueryPage
    expect(parseJs8Context(page).history.K2ABC.count).toBe(0)
    expect(parseJs8Context(page).history.W1AW.grid).toBe('')
    expect(parseJs8Context(page).history.N0CALL).toBeUndefined()
    expect(() => parseJs8Context({...page,meta:{...(page.meta as object),capturedAgeMs:60_000}})).toThrow()
    expect(() => parseJs8Context({...page,retained:1})).toThrow()
  })
})
