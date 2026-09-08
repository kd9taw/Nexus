// v2 mirrors tempo-app::remote_monitor. Validate before any unknown source reaches a view.
export const POLL_MS = 500
export const STALE_MS = 3000
export const MAX_FRAME_BYTES = 16384
export const MEASUREMENT_STALE_MS = 5000
export const MODE_STALE_MS = 10000
export type ReadAge = { connectionGeneration: number; readSequence: number; ageMs: number }
export type SourceKind = 'native' | 'fixture'
export type MonitorAmplifier = {
  reading: ReadAge | null
  family: string; model: string; followBand: boolean; linked: boolean; reason: string
  operate: boolean | null; transmitting: boolean | null; outputWatts: number | null
  bandLabel: string | null; swr: number | null; swrAtu: number | null
  volts: number | null; amps: number | null; temp: number | null; tempCelsius: boolean
  alarm: string; alarmRaised: boolean; warning: string; warningRaised: boolean
  kpaFault: number | null
}
export type MonitorFrame = {
  version: 2; source: SourceKind; epoch: string; sequence: number; generatedAtMs: number
  station: {
    call: string; grid: string
    radio: {
      id: number; name: string; dialMhz: number | null; band: string; mode: string
      rigMode: string | null; catConnected: boolean | null; rigKeyed: boolean | null
      nexusBusy: boolean
      rigDialMhz: number | null
      readings: { cat: ReadAge | null; dial: ReadAge | null; mode: ReadAge | null; ptt: ReadAge | null }
    }
    amplifier: MonitorAmplifier | null
  }
}

type Check = (value: unknown) => boolean
const text: Check = (v) => typeof v === 'string' && Array.from(v).length <= 64
const bool: Check = (v) => typeof v === 'boolean'
const finite: Check = (v) => typeof v === 'number' && Number.isFinite(v)
const integer = (min: number, max: number): Check =>
  (v) => typeof v === 'number' && Number.isSafeInteger(v) && v >= min && v <= max
const nullable = (check: Check): Check => (v) => v === null || check(v)
const shape = (checks: Record<string, Check>): Check => (v) => {
  if (!v || typeof v !== 'object' || Array.isArray(v)) return false
  const record = v as Record<string, unknown>
  const keys = Object.keys(record)
  return keys.length === Object.keys(checks).length &&
    keys.every((key) => Object.prototype.hasOwnProperty.call(checks, key) && checks[key](record[key]))
}
const readAge = nullable(shape({
  connectionGeneration: integer(1, Number.MAX_SAFE_INTEGER), readSequence: integer(1, Number.MAX_SAFE_INTEGER),
  ageMs: integer(0, Number.MAX_SAFE_INTEGER),
}))
const amplifier = shape({
  reading: readAge,
  family: text, model: text, followBand: bool, linked: bool, reason: text,
  operate: nullable(bool), transmitting: nullable(bool), outputWatts: nullable(integer(0, 65535)),
  bandLabel: nullable(text), swr: nullable(finite), swrAtu: nullable(finite),
  volts: nullable(finite), amps: nullable(finite), temp: nullable(integer(-32768, 32767)),
  tempCelsius: bool, alarm: text, alarmRaised: bool, warning: text, warningRaised: bool,
  kpaFault: nullable(integer(0, 255)),
})
const frame = shape({
  version: (v) => v === 2,
  source: (v) => v === 'native' || v === 'fixture',
  epoch: (v) => text(v) && v !== '', sequence: integer(1, Number.MAX_SAFE_INTEGER),
  generatedAtMs: integer(0, Number.MAX_SAFE_INTEGER),
  station: shape({
    call: text, grid: text, amplifier: nullable(amplifier),
    radio: shape({
      id: integer(0, 4294967295), name: text, dialMhz: nullable(finite), band: text, mode: text,
      rigMode: nullable(text), catConnected: nullable(bool), rigKeyed: nullable(bool), nexusBusy: bool,
      rigDialMhz: nullable(finite), readings: shape({ cat: readAge, dial: readAge, mode: readAge, ptt: readAge }),
    }),
  }),
})

export function parseFrame(value: unknown, source: SourceKind): MonitorFrame {
  // Shape/string bounds are checked BEFORE serialization; an unexpected history array
  // cannot turn this byte-budget check into a scan of the station's entire history.
  if (!frame(value)) throw new Error('invalidMonitorFrame')
  const result = value as MonitorFrame
  const radio = result.station.radio
  const pairs = [[radio.catConnected, radio.readings.cat], [radio.rigDialMhz, radio.readings.dial],
    [radio.rigMode, radio.readings.mode], [radio.rigKeyed, radio.readings.ptt]] as const
  if (pairs.some(([value, age]) => (value === null) !== (age === null)) ||
    new Set(pairs.flatMap(([, age]) => age ? [age.connectionGeneration] : [])).size > 1) {
    throw new Error('invalidMonitorFrame')
  }
  const amp = result.station.amplifier
  if (amp && !amp.reading && (amp.linked || amp.operate !== null || amp.transmitting !== null ||
    amp.outputWatts !== null || amp.bandLabel !== null || amp.swr !== null || amp.swrAtu !== null ||
    amp.volts !== null || amp.amps !== null || amp.temp !== null || amp.kpaFault !== null || amp.alarmRaised || amp.warningRaised)) {
    throw new Error('invalidMonitorFrame')
  }
  if (result.source !== source || new TextEncoder().encode(JSON.stringify(result)).length > MAX_FRAME_BYTES) {
    throw new Error('invalidMonitorFrame')
  }
  return result
}

// Age keeps advancing between publications, including while a source returns the
// same cached sequence. Wall clocks on the browser and shack need not agree.
export function ageFrame(frame: MonitorFrame, elapsedMs: number): MonitorFrame {
  const age = (stamp: ReadAge | null, limit = MEASUREMENT_STALE_MS): ReadAge | null => {
    if (!stamp || stamp.ageMs + elapsedMs >= limit) return null
    return { ...stamp, ageMs: stamp.ageMs + Math.max(0, elapsedMs) }
  }
  const radio = frame.station.radio
  const readings = { cat: age(radio.readings.cat), dial: age(radio.readings.dial),
    mode: age(radio.readings.mode, MODE_STALE_MS), ptt: age(radio.readings.ptt) }
  const amp = frame.station.amplifier
  const reading = age(amp?.reading ?? null)
  const amplifier = !amp ? null : reading ? { ...amp, reading } : {
    ...amp, reading: null, linked: false, operate: null, transmitting: null, outputWatts: null,
    bandLabel: null, swr: null, swrAtu: null, volts: null, amps: null, temp: null,
    alarm: '', alarmRaised: false, warning: '', warningRaised: false, kpaFault: null,
  }
  return { ...frame, station: { ...frame.station, amplifier, radio: { ...radio, readings,
    catConnected: readings.cat ? radio.catConnected : null, rigDialMhz: readings.dial ? radio.rigDialMhz : null,
    rigMode: readings.mode ? radio.rigMode : null, rigKeyed: readings.ptt ? radio.rigKeyed : null,
  } } }
}

export class FrameOrder {
  private last: MonitorFrame | null = null
  private retired = new Set<string>()

  accept(next: MonitorFrame): boolean {
    if (this.retired.has(next.epoch)) return false
    if (this.last?.epoch === next.epoch && next.sequence <= this.last.sequence) return false
    if (this.last && this.last.epoch !== next.epoch) {
      // Bound memory and fail closed after repeated process changes. Reopening the
      // observer starts a new source session; never evict an epoch and allow replay.
      if (this.retired.size === 16) throw new Error('monitorSourceChangedTooOften')
      this.retired.add(this.last.epoch)
    }
    this.last = next
    return true
  }
}
