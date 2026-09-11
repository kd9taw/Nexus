import type { ApplicationTransport } from '../applicationTransport'
import type { ApplicationClient } from './application-client'
import type { OperationClient } from './operation-client'
import { stationAction, type StationAction, type ControlContext } from './station-operation'
import { APPLICATION_TIMEOUT_MS } from './application-protocol'
import type { ApplicationCommand } from './application-protocol'
import { readBandChoices } from './band-choices'

/** Adapt only the reviewed local gestures. The station receives typed intents,
 * never an invoke name; every other command remains behind the read allowlist. */
export function controlTransport(reads: ApplicationTransport, client: ApplicationClient, operations: OperationClient): ApplicationTransport {
  return {
    kind: 'remote',
    async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
      let action: StationAction | null = null, read = '', stopped = false
      switch (command) {
        case 'halt_tx':
          if (args && Object.keys(args).length) throw Error('invalidOperation')
          await operations.stopTransmit()
          stopped = true
          read = 'get_snapshot'
          break
        case 'start_cq': case 'set_tx_enabled': {
          const key = command === 'start_cq' ? 'dir' : 'enabled'
          if (!args || Object.keys(args).length !== 1 || !(key in args)) throw Error('invalidOperation')
          const state = operations.getSnapshot().state
          if (!state?.transmitEpoch) throw Error('localPermissionRequired')
          const snapshot = await reads.invoke<import('../types').AppSnapshot>('get_snapshot')
          const common = { expectedTier: snapshot.link.tier, transmitEpoch: state.transmitEpoch }
          action = stationAction(command === 'start_cq' ? { action: 'ft.cq', ...common, direction: args.dir }
            : { action: 'ft.txEnabled', ...common, on: args.enabled })
          read = 'get_snapshot'
          break
        }
        case 'set_active_radio':
          if (!args || Object.keys(args).some(k => k !== 'id')) throw Error('invalidOperation')
          action = stationAction({ action: 'radio.select', radioId: args.id })
          read = 'get_snapshot'
          break
        case 'work_spot':
          if (!args || Object.keys(args).some(k => !['mode', 'freqMhz', 'band', 'call', 'tier'].includes(k)) || (args.tier !== null && args.tier !== undefined)) throw Error('applicationUnsupported')
          action = stationAction({ action: 'radio.workSpot', mode: args.mode, dialMhz: args.freqMhz, band: args.band, call: args.call })
          read = 'get_snapshot'
          break
        case 'get_licensed_band_plan': {
          if (!args || Object.keys(args).length !== 1 || typeof args.mode !== 'string' || !['cw', 'phone'].includes(args.mode)) throw Error('applicationUnsupported')
          return readBandChoices(await reads.invoke('get_settings'), args.mode as 'cw' | 'phone') as T
        }
        case 'pick_band':
          action = stationAction({ action: 'radio.band', band: args?.band, mode: args?.mode })
          if (!args || Object.keys(args).some(k => !['band', 'mode'].includes(k))) throw Error('invalidOperation')
          read = 'get_snapshot'
          break
        case 'set_tier':
          action = stationAction({ action: 'radio.tier', tier: args?.tier })
          if (!args || Object.keys(args).some(k => k !== 'tier')) throw Error('invalidOperation')
          read = 'get_snapshot'
          break
        case 'set_operating_mode':
          action = stationAction({ action: 'radio.mode', mode: args?.mode, followFrequency: args?.followFreq })
          if (!args || Object.keys(args).some(k => !['mode', 'followFreq'].includes(k))) throw Error('invalidOperation')
          read = 'get_snapshot'
          break
        case 'set_frequency':
          action = stationAction({ action: 'radio.frequency', dialMhz: args?.dialMhz, band: args?.band, sideband: args?.mode })
          // Refuse extra arguments instead of silently dropping an unreviewed intent.
          if (!args || Object.keys(args).some(k => !['dialMhz', 'band', 'mode'].includes(k))) throw Error('invalidOperation')
          read = 'get_snapshot'
          break
        case 'rtty_arm': case 'psk_arm': case 'sstv_arm': {
          const receiver = command.slice(0, -4)
          action = stationAction({ action: 'decoder.arm', receiver, ...args })
          read = `get_${receiver}_state`
          break
        }
        case 'cw_clear': case 'rtty_clear': case 'psk_clear':
          action = stationAction({ action: 'decoder.clear', receiver: command.split('_')[0], ...args })
          read = command === 'cw_clear' ? '' : `get_${command.split('_')[0]}_state`
          break
        case 'rtty_afc_reset': case 'psk_afc_reset':
          action = stationAction({ action: 'decoder.afcReset', receiver: command.split('_')[0], ...args })
          read = `get_${command.split('_')[0]}_state`
          break
        case 'rtty_net': case 'psk_net':
          action = stationAction({ action: 'decoder.net', receiver: command.split('_')[0], ...args })
          read = `get_${command.split('_')[0]}_state`
          break
        case 'psk_set_mode':
          action = stationAction({ action: 'decoder.pskMode', ...args, mode: typeof args?.mode === 'string' ? args.mode.toUpperCase() : args?.mode })
          read = 'get_psk_state'
          break
      }
      if (!action && !stopped) return reads.invoke<T>(command, args)
      let displayed: ControlContext | undefined
      if (action?.action === 'radio.select') {
        const context = operations.getSnapshot().state?.controls?.context
        if (!context) throw Error('readingUnavailable')
        displayed = { ...context }
        const snapshot = await reads.invoke<import('../types').AppSnapshot>('get_snapshot')
        if (snapshot.activeRadioId !== displayed.radioId || !snapshot.radios?.some(r => r.id === action.radioId)) throw Error('readingUnavailable')
      }
      if (action) {
        const result = await operations.control(action, displayed)
        if (result.outcome !== 'applied') throw Error(result.outcome === 'rejected' ? result.reason : 'operationUnknown')
        if (action?.action === 'radio.workSpot' && result.evidence !== 'radioReadback') throw Error('operationUnknown')
        if (action?.action === 'radio.select' && result.evidence !== 'radioReadback' && !(displayed?.radioId === action.radioId && result.evidence === 'stationState')) throw Error('operationUnknown')
      }
      if (!read) return undefined as T
      // The UI receives a later station sample, not an optimistic local copy or
      // the pre-command snapshot still in the stream's cache.
      const after = performance.now()
      // Keep Settings in the subscription even when its panel is closed.
      // A cache-age check alone cannot request a new station sample.
      if (action?.action === 'radio.select') await reads.invoke('get_settings')
      while (performance.now() - after < APPLICATION_TIMEOUT_MS) {
        if (client.age(read as ApplicationCommand) <= performance.now() - after && (action?.action !== 'radio.select' || client.age('get_settings') <= performance.now() - after)) {
          const value = await reads.invoke<T>(read)
          if (action?.action === 'radio.workSpot') {
            const radio = (value as import('../types').AppSnapshot)?.radio
            if (!radio || Math.round(radio.dialMhz * 1e6) !== Math.round(action.dialMhz * 1e6) || radio.operatingMode?.toLowerCase() !== action.mode) throw Error('readingUnavailable')
          }
          if (action?.action === 'radio.select') {
            const snapshot = value as import('../types').AppSnapshot
            const settings = await reads.invoke<import('../types').Settings>('get_settings')
            if (snapshot.activeRadioId !== action.radioId || settings.activeRadio !== action.radioId) throw Error('readingUnavailable')
          }
          return value
        }
        await new Promise(resolve => setTimeout(resolve, 50))
      }
      throw Error('readingUnavailable')
    }
  }
}
