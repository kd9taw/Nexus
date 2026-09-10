import type { ApplicationTransport } from '../applicationTransport'
import type { ApplicationClient } from './application-client'
import type { OperationClient } from './operation-client'
import { stationAction, type StationAction } from './station-operation'
import { APPLICATION_TIMEOUT_MS } from './application-protocol'
import type { ApplicationCommand } from './application-protocol'

/** Adapt only the reviewed local gestures. The station receives typed intents,
 * never an invoke name; every other command remains behind the read allowlist. */
export function controlTransport(reads: ApplicationTransport, client: ApplicationClient, operations: OperationClient): ApplicationTransport {
  return {
    kind: 'remote',
    async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
      let action: StationAction | null = null, read = ''
      switch (command) {
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
      if (!action) return reads.invoke<T>(command, args)
      const result = await operations.control(action)
      if (result.outcome !== 'applied') throw Error(result.outcome === 'rejected' ? result.reason : 'operationUnknown')
      if (!read) return undefined as T
      // The UI receives a later station sample, not an optimistic local copy or
      // the pre-command snapshot still in the stream's cache.
      const after = performance.now()
      while (performance.now() - after < APPLICATION_TIMEOUT_MS) {
        if (client.age(read as ApplicationCommand) <= performance.now() - after) return reads.invoke<T>(read)
        await new Promise(resolve => setTimeout(resolve, 50))
      }
      throw Error('readingUnavailable')
    }
  }
}
