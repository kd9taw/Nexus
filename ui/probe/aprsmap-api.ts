// The APRS answers the APRS map probe adds to the waterfalls probe's api (`scripts/browser-probe --desktop aprsmap`).
//
// aprsmap-probe.mjs serves probe/waterfalls-api.ts with this file appended in place of src/api.ts. A small local
// net around the snapshot's grid (EN52): a car that has driven a few kilometres (its track), a fixed station, a
// weather station, a digipeater and an internet-only station. Nothing here is a radio; nothing transmits.
const aprsNow = () => Math.floor(Date.now() / 1000)
function aprsStation(call: string, lat: number, lon: number, code: string, over: Record<string, unknown> = {}): any {
  const now = aprsNow()
  return {
    call, lat, lon, symbolTable: '/', symbolCode: code, kind: 'position', text: '', speedKnots: null, courseDeg: null,
    path: ['WIDE1-1'], raw: '', lastHeardUnix: now - 40, lastRfUnix: now - 40, lastInetUnix: null, sourceKind: 'rf',
    packets: 4, firstHeardUnix: now - 1800, wx: null, ...over,
  }
}
export async function getAprsStations(): Promise<any> {
  const now = aprsNow()
  return {
    ttlMin: 60,
    fadeAfterMin: 20,
    stations: [
      aprsStation('KD9TAW-9', 42.12, -87.95, '>', {
        speedKnots: 35, courseDeg: 45,
        track: [
          { lat: 42.05, lon: -88.05, atUnix: now - 900 },
          { lat: 42.08, lon: -88.01, atUnix: now - 600 },
          { lat: 42.1, lon: -87.98, atUnix: now - 300 },
          { lat: 42.12, lon: -87.95, atUnix: now - 40 },
        ],
      }),
      aprsStation('W9FIX', 41.98, -87.9, '-'),
      aprsStation('W9WX', 42.02, -88.12, '_'),
      aprsStation('W9DIG-1', 42.2, -88.2, '#'),
      aprsStation('N9NET', 41.9, -87.7, '-', { sourceKind: 'inet', lastRfUnix: null, lastInetUnix: now - 20 }),
    ],
  }
}
export async function getAprsHeard(): Promise<any> { return [] }
export async function getAprsHealth(): Promise<any> {
  const now = aprsNow()
  return { arm: 'explicit', audioPeak: 0.2, lastAudioUnix: now, drains: 40, framesSeen: 5, framesDecoded: 5, lastDecodeUnix: now - 40,
    lastFrameUnix: now - 40 }
}
export async function getAprsIsStatus(): Promise<any> {
  return { enabled: false, connected: false, verified: false, packets: 0, lastPacketUnix: null, uplinkEnabled: false, uploaded: 0,
    gateRejected: 0, lastReject: null }
}
export async function getAprsTxNotice(): Promise<any> { return null }
export async function aprsAutoArm(): Promise<any> { return true }
export async function aprsArm(): Promise<any> { return [] }
// Entering APRS tunes the 2 m channel; the waterfalls api's snapshot stands for the answer.
export async function aprsTune(): Promise<any> { return snapshot }
