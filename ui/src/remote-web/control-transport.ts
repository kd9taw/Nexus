import type { ApplicationTransport } from '../applicationTransport'
import type { ApplicationClient } from './application-client'
import { OperationFailure, type OperationClient } from './operation-client'
import { stationAction, type StationAction, type ControlContext } from './station-operation'
import { APPLICATION_TIMEOUT_MS } from './application-protocol'
import type { ApplicationCommand } from './application-protocol'
import { readBandChoices } from './band-choices'

type TuningAction = Extract<StationAction, { action: 'radio.split' | 'radio.xit' | 'radio.vfo' | 'radio.rit' }>
const isTuning = (action: StationAction): action is TuningAction => ['radio.split', 'radio.xit', 'radio.vfo', 'radio.rit'].includes(action.action)
/** The station applies these as one-shots its radio loop writes; a later sample must show the value. */
function tuningShown(action: TuningAction, radio: import('../types').RadioStatus | undefined): boolean {
  if (!radio) return false
  switch (action.action) {
    case 'radio.split': return action.txMhz === null ? radio.splitTxMhz == null
      : radio.splitTxMhz != null && Math.abs(Math.round(radio.splitTxMhz * 1e6) - Math.round(action.txMhz * 1e6)) <= 1
    case 'radio.vfo': return (radio.activeVfo === 'B' ? 'B' : 'A') === action.vfo
    case 'radio.rit': return radio.ritHz === action.hz
    case 'radio.xit': return radio.xitHz === action.hz
  }
}

/** Adapt only the reviewed local gestures. The station receives typed intents,
 * never an invoke name; every other command remains behind the read allowlist. */
export function controlTransport(reads: ApplicationTransport, client: ApplicationClient, operations: OperationClient): ApplicationTransport {
  return {
    kind: 'remote',
    async invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
      let action: StationAction | null = null, read = '', stopped = false
      let qsoResult: 'logged' | 'pending' | 'none' | null = null
      let captured: ReturnType<OperationClient['prepareControl']> | undefined
      let displayed: ControlContext | undefined
      // Everything up to handing the action to the operation client runs before any request
      // leaves this browser, so a throw here reached nothing at the station. Stop keeps its own
      // error: its request can already have been sent.
      try {
        switch (command) {
          case 'set_rx_offset': case 'set_skip_tx1': {
            const field = command === 'set_rx_offset' ? 'hz' : 'enabled', keys = [field, 'expectedTier', 'expected']
            if (!args || Object.keys(args).length !== keys.length || Object.keys(args).some(k => !keys.includes(k))) throw Error('invalidOperation')
            const gesture = structuredClone(args)
            // A typed or dragged FT value can be committed during a brief control lapse: keep the
            // gesture as displayed, wait for control to be current again (refused as not sent if it
            // stays stale), then capture the station state it is sent with.
            await operations.awaitCurrent()
            const state = operations.getSnapshot().state
            if (!state?.transmitEpoch) throw Error('localPermissionRequired')
            captured = operations.prepareControl()
            action = stationAction({ action: 'ft.runtime', expectedTier: gesture.expectedTier, expected: gesture.expected, transmitEpoch: state.transmitEpoch,
              change: command === 'set_rx_offset' ? { kind: 'rxOffset', hz: gesture.hz } : { kind: 'skipTx1', on: gesture.enabled } })
            read = 'get_snapshot'
            break
          }
          case 'set_tx_offset': case 'set_ft_both_offsets': case 'set_hold_tx_freq': case 'set_tx_even': case 'set_tx_cycle_auto': {
            const field = command === 'set_tx_offset' || command === 'set_ft_both_offsets' ? 'hz' : command === 'set_hold_tx_freq' ? 'on' : command === 'set_tx_even' ? 'even' : 'auto'
            const keys = [field, 'expectedTier', 'expected']
            if (!args || Object.keys(args).length !== keys.length || Object.keys(args).some(k => !keys.includes(k))) throw Error('invalidOperation')
            const gesture = structuredClone(args)
            // A typed or dragged FT value can be committed during a brief control lapse: keep the
            // gesture as displayed, wait for control to be current again (refused as not sent if it
            // stays stale), then capture the station state it is sent with.
            await operations.awaitCurrent()
            const state = operations.getSnapshot().state
            if (!state?.transmitEpoch) throw Error('localPermissionRequired')
            captured = operations.prepareControl()
            const kind = command === 'set_tx_offset' ? 'txOffset' : command === 'set_ft_both_offsets' ? 'bothOffsets' : command === 'set_hold_tx_freq' ? 'hold' : command === 'set_tx_even' ? 'even' : 'auto'
            action = stationAction({ action: 'ft.setting', expectedTier: gesture.expectedTier, transmitEpoch: state.transmitEpoch,
              expected: gesture.expected, change: { kind, [field]: gesture[field] } })
            read = 'get_snapshot'
            break
          }
          case 'log_current_qso': case 'confirm_pending_log': case 'discard_pending_log': {
            const keys = command === 'log_current_qso' ? ['expectedKey', 'expectedTier', 'expectedQso'] : command === 'confirm_pending_log' ? ['record', 'expectedKey'] : ['expectedKey']
            if (!args || Object.keys(args).length !== keys.length || Object.keys(args).some(k => !keys.includes(k))) throw Error('invalidOperation')
            const gesture = structuredClone(args)
            captured = operations.prepareControl()
            if (command === 'log_current_qso') {
              const q = gesture.expectedQso as import('../types').QsoStatus | null
              if (!q) throw Error('staleContext')
              action = stationAction({ action: 'qso.logCurrent', expectedKey: gesture.expectedKey, expectedTier: gesture.expectedTier,
                expectedQso: { dxcall: q.dxcall, state: q.state, txNow: q.txNow ?? null, cqRunning: q.cqRunning ?? false } })
            } else if (command === 'confirm_pending_log') {
              const record = gesture.record as import('../types').LoggedQso | null
              if (!record) throw Error('invalidOperation')
              action = stationAction({ action: 'qso.confirm', expectedKey: gesture.expectedKey,
                edits: { call: record.call, grid: record.grid, rstSent: record.rstSent, rstRcvd: record.rstRcvd } })
            } else action = stationAction({ action: 'qso.discard', expectedKey: gesture.expectedKey })
            read = 'get_snapshot'
            break
          }
          case 'override_next_tx': case 'qso_resend': case 'qso_freetext': case 'set_mode': {
            const keys = command === 'override_next_tx' ? ['call', 'grid', 'text', 'expectedQso'] : command === 'qso_resend' ? ['expectedQso'] : [command === 'qso_freetext' ? 'text' : 'mode', 'expectedQso']
            if (!args || Object.keys(args).length !== keys.length || Object.keys(args).some(key => !keys.includes(key))) throw Error('invalidOperation')
            if (command === 'set_mode' && args?.mode !== 'qso-monitor') throw Error('applicationUnsupported')
            const gesture = structuredClone(args)
            const qso = gesture.expectedQso as import('../types').QsoStatus | null
            if (!qso) throw Error('staleContext')
            const state = operations.getSnapshot().state
            if (!state?.transmitEpoch) throw Error('localPermissionRequired')
            captured = operations.prepareControl()
            const snapshot = await reads.invoke<import('../types').AppSnapshot>('get_snapshot')
            action = stationAction({ action: command === 'override_next_tx' ? 'ft.message' : 'ft.exchange', expectedTier: snapshot.link.tier,
              transmitEpoch: state.transmitEpoch,
              expectedQso: { dxcall: qso.dxcall, state: qso.state, txNow: qso.txNow ?? null, cqRunning: qso.cqRunning ?? false },
              ...(command === 'override_next_tx' ? { call: gesture.call, grid: gesture.grid, text: gesture.text } :
                { change: command === 'qso_resend' ? { kind: 'resend' } : command === 'set_mode' ? { kind: 'monitor' } : { kind: 'freeText', text: gesture.text } }) })
            read = 'get_snapshot'
            break
          }
          case 'halt_tx':
            if (args && Object.keys(args).length) throw Error('invalidOperation')
            await operations.stopTransmit()
            stopped = true
            read = 'get_snapshot'
            break
          case 'call_station': {
            if (!args || Object.keys(args).length !== 5 || Object.keys(args).some(k => !['call', 'grid', 'message', 'snr', 'freq'].includes(k))) throw Error('invalidOperation')
            const state = operations.getSnapshot().state
            if (!state?.transmitEpoch) throw Error('localPermissionRequired')
            const snapshot = await reads.invoke<import('../types').AppSnapshot>('get_snapshot')
            action = stationAction({ action: 'ft.call', expectedTier: snapshot.link.tier,
              transmitEpoch: state.transmitEpoch, selection: args })
            read = 'get_snapshot'
            break
          }
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
            if (!args || Object.keys(args).some(k => !['mode', 'freqMhz', 'band', 'call', 'tier'].includes(k))) throw Error('applicationUnsupported')
            if (args.tier === null || args.tier === undefined) action = stationAction({ action: 'radio.workSpot', mode: args.mode, dialMhz: args.freqMhz, band: args.band, call: args.call })
            // FT8/FT4 carries its tier in its own action, never as a workSpot field an older desktop cannot parse.
            else if (args.mode === 'digital') action = stationAction({ action: 'radio.workDigitalSpot', tier: args.tier, dialMhz: args.freqMhz, band: args.band, call: args.call })
            else throw Error('applicationUnsupported')
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
          case 'set_ai_cw': {
            if (!args || Object.keys(args).length !== 1 || typeof args.on !== 'boolean') throw Error('invalidOperation')
            // Bind the choice to the switch position the station last reported.
            const snapshot = await reads.invoke<import('../types').AppSnapshot>('get_snapshot')
            if (typeof snapshot?.aiCw?.enabled !== 'boolean') throw Error('readingUnavailable')
            action = stationAction({ action: 'decoder.aiCw', expectedOn: snapshot.aiCw.enabled, on: args.on })
            read = 'get_snapshot'
            break
          }
          case 'redecode': {
            if (args && Object.keys(args).length) throw Error('invalidOperation')
            const snapshot = await reads.invoke<import('../types').AppSnapshot>('get_snapshot')
            action = stationAction({ action: 'decoder.redecode', expectedTier: snapshot?.link?.tier })
            read = 'get_snapshot'
            break
          }
          case 'set_split': case 'set_rit': case 'set_xit': case 'set_vfo': {
            const key = command === 'set_split' ? 'txMhz' : command === 'set_vfo' ? 'vfo' : 'hz'
            if (!args || Object.keys(args).length !== 1 || !(key in args)) throw Error('invalidOperation')
            // Bind the change to the value the station last reported, in whole Hz for a split.
            const radio = (await reads.invoke<import('../types').AppSnapshot>('get_snapshot'))?.radio
            if (!radio) throw Error('readingUnavailable')
            const whole = (mhz: unknown) => typeof mhz === 'number' ? Math.round(mhz * 1e6) / 1e6 : mhz
            action = stationAction(command === 'set_split' ? { action: 'radio.split', expectedTxMhz: radio.splitTxMhz == null ? null : whole(radio.splitTxMhz), txMhz: args.txMhz === null ? null : whole(args.txMhz) }
              : command === 'set_vfo' ? { action: 'radio.vfo', expectedVfo: radio.activeVfo === 'B' ? 'B' : 'A', vfo: args.vfo }
              : { action: command === 'set_rit' ? 'radio.rit' : 'radio.xit', expectedHz: (command === 'set_rit' ? radio.ritHz : radio.xitHz) ?? 0, hz: args.hz })
            read = 'get_snapshot'
            break
          }
          // The desktop has no swap button; a browser may not reach one either.
          case 'swap_vfo':
            throw Error('applicationUnsupported')
        }
        if (action?.action === 'radio.select') {
          const context = operations.getSnapshot().state?.controls?.context
          if (!context) throw Error('readingUnavailable')
          displayed = { ...context }
          const radioId = action.radioId
          const snapshot = await reads.invoke<import('../types').AppSnapshot>('get_snapshot')
          if (snapshot.activeRadioId !== displayed.radioId || !snapshot.radios?.some(r => r.id === radioId)) throw Error('readingUnavailable')
        }
      } catch (error) {
        if (command === 'halt_tx' || error instanceof OperationFailure) throw error
        throw new OperationFailure(error instanceof Error ? error.message : 'invalidOperation', false)
      }
      if (!action && !stopped) return reads.invoke<T>(command, args)
      if (action) {
        const result = await (captured ? captured(action) : operations.control(action, displayed))
        if (action.action === 'qso.logCurrent' && result.outcome === 'rejected' && ['noEligibleContact', 'alreadyPresent'].includes(result.reason)) qsoResult = 'none'
        else if (result.outcome === 'rejected' && result.reason === 'stationBusy') throw new OperationFailure(result.reason, true, true)
        else if (result.outcome !== 'applied') throw Error(result.outcome === 'rejected' ? result.reason : 'operationUnknown')
        if (action.action === 'ft.runtime' && (result.outcome !== 'applied' || result.evidence !== (action.change.kind === 'rxOffset' ? 'settingsSaved' : 'stationState'))) throw Error('operationUnknown')
        if (action.action === 'ft.setting' && (result.outcome !== 'applied' || result.evidence !== (action.change.kind === 'auto' ? 'stationState' : 'settingsSaved'))) throw Error('operationUnknown')
        if (action.action === 'qso.logCurrent' && result.outcome === 'applied') {
          if (result.evidence === 'fileSynced') qsoResult = 'logged'
          else if (result.evidence === 'pendingConfirmationSynced') qsoResult = 'pending'
          else throw Error('operationUnknown')
        }
        if ((action.action === 'qso.confirm' || action.action === 'qso.discard') &&
          (result.outcome !== 'applied' || result.evidence !== (action.action === 'qso.confirm' ? 'fileSynced' : 'pendingDiscarded'))) throw Error('operationUnknown')
        if (action?.action === 'radio.workSpot' && result.outcome === 'applied' && result.evidence !== 'radioReadback') throw Error('operationUnknown')
        if (action.action === 'radio.workDigitalSpot' && (result.outcome !== 'applied' || result.evidence !== 'radioReadback')) throw Error('operationUnknown')
        if (action.action === 'decoder.aiCw' && (result.outcome !== 'applied' || result.evidence !== 'settingsSaved')) throw Error('operationUnknown')
        if (action.action === 'decoder.redecode' && (result.outcome !== 'applied' || result.evidence !== 'receiverState')) throw Error('operationUnknown')
        if (isTuning(action) && (result.outcome !== 'applied' || result.evidence !== 'stationState')) throw Error('operationUnknown')
        if (action?.action === 'radio.select' && result.outcome === 'applied' && result.evidence !== 'radioReadback' && !(displayed?.radioId === action.radioId && result.evidence === 'stationState')) throw Error('operationUnknown')
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
          if (action?.action === 'radio.workDigitalSpot') {
            const snapshot = value as import('../types').AppSnapshot
            if (!snapshot?.radio || Math.round(snapshot.radio.dialMhz * 1e6) !== Math.round(action.dialMhz * 1e6) || snapshot.radio.operatingMode?.toLowerCase() !== 'digital' || snapshot.link?.tier !== action.tier) throw Error('readingUnavailable')
          }
          if (action?.action === 'decoder.aiCw' && (value as import('../types').AppSnapshot)?.aiCw?.enabled !== action.on) throw Error('readingUnavailable')
          if (action && isTuning(action) && !tuningShown(action, (value as import('../types').AppSnapshot)?.radio)) throw Error('readingUnavailable')
          if (action?.action === 'radio.select') {
            const snapshot = value as import('../types').AppSnapshot
            const settings = await reads.invoke<import('../types').Settings>('get_settings')
            if (snapshot.activeRadioId !== action.radioId || settings.activeRadio !== action.radioId) throw Error('readingUnavailable')
          }
          if (qsoResult) return { logged: qsoResult === 'logged', pending: qsoResult === 'pending', snapshot: value } as T
          return value
        }
        await new Promise(resolve => setTimeout(resolve, 50))
      }
      throw Error('readingUnavailable')
    }
  }
}
