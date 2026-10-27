// @vitest-environment jsdom
//
// Settings ▸ Radio ▸ Transmit limits & sharing ▸ Share by TCI: Nexus as a TCI server for the
// programs on this computer.
//
// Pinned here: it is off as it comes and shows nothing past its own switch; the address is the
// one the station reports it bound, never the port in the form, and a port the station could not
// bind shows the station's own sentence in the address's place; each switch and the port save
// through the form, and a port outside 1–65535 never does; "Programs on TCI may transmit" is drawn
// and set only at the station, and from afar its row says "Set at the station" with no switch at
// all; the slot-timed refusal is said beside it; the programs connected are listed with what each
// is doing and the last thing refused it, each with a Disconnect for that program only, which is
// not offered from afar; what is shown follows what the station reports; the port and the status
// types mirror the station's own; the pt-BR kit carries every string; and the settings search
// finds it.
import { describe, it, expect, vi, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, within } from '@testing-library/react'
import { readFileSync } from 'node:fs'
import { fileURLToPath } from 'node:url'
import {
  TciServerSettings,
  TCI_SERVER_DEFAULT_PORT,
  sharePortClash,
  type TciServerKey,
} from './TciServerSettings'
import { EN } from '../i18n/en'
import { t } from '../i18n'
import { searchSettings } from '../settings/registry'
import type { Settings, TciProgramStatus, TciServerStatus } from '../types'
import defaultSettings from './__fixtures__/defaultSettings.json'

afterEach(cleanup)

const form = (over: Partial<Settings> = {}) =>
  ({
    mycall: 'KD9TAW',
    catBroker: true,
    catBrokerPort: 4532,
    tciServer: false,
    tciServerPort: 40001,
    tciServerTx: false,
    tciServerCw: true,
    tciServerSpots: true,
    ...over,
  }) as Settings

/** At the station nothing is the station's alone; from afar each of the five is, and the panel's
 *  own `setAtStation` says so. */
const AT_THE_STATION = { locked: () => false, setAtStation: () => null }
const FROM_AFAR = {
  locked: () => true,
  setAtStation: (key: TciServerKey) => (
    <span className="settings-hint" data-set-at-station={key}>
      {t('settings.stationOnly.hint')}
    </span>
  ),
}

function setup(
  over: Partial<Settings> = {},
  status?: TciServerStatus | null,
  where: typeof AT_THE_STATION | typeof FROM_AFAR = AT_THE_STATION,
) {
  const onSwitch = vi.fn()
  const onPort = vi.fn()
  const onDisconnect = vi.fn()
  const view = render(
    <TciServerSettings
      form={form(over)}
      status={status}
      onSwitch={onSwitch}
      onPort={onPort}
      onDisconnect={onDisconnect}
      {...where}
    />,
  )
  return { ...view, onSwitch, onPort, onDisconnect }
}

const serving = (over: Partial<TciServerStatus> = {}): TciServerStatus => ({
  port: 40001,
  error: null,
  programs: [],
  ...over,
})

const WSJTX: TciProgramStatus = {
  id: 1,
  peer: '127.0.0.1:53412',
  audio: true,
  keyed: false,
  refused: 'programs on TCI may not transmit',
}
const LOGGER: TciProgramStatus = { id: 2, peer: '[::1]:53500', audio: false, keyed: true, refused: null }

/** A catalog sentence as it reads on screen: the markers the call site turns into tags are gone. */
const plain = (key: keyof typeof EN) => String(EN[key]).replace(/<\/?[a-z]+>/g, '')

const theSwitch = (key: keyof typeof EN) => screen.queryByRole('switch', { name: EN[key] as string })
const master = () => screen.getByRole('switch', { name: EN['settings.transmit.tciServer.label'] })
const portBox = () => screen.queryByLabelText(EN['settings.transmit.tciServer.port.label'], { exact: false })
const address = () => document.querySelector('[data-tci-server-address]')
const programRows = () => screen.queryAllByRole('listitem')

describe('Share by TCI', () => {
  it('is off as it comes, and shows nothing past its own switch', () => {
    const { container } = setup({ tciServer: undefined })
    expect(master().getAttribute('aria-checked')).toBe('false')
    expect(container.textContent).toContain(plain('settings.transmit.tciServer.hint.off'))
    expect(address()).toBeNull()
    expect(portBox()).toBeNull()
    expect(theSwitch('settings.transmit.tciServer.tx.label')).toBeNull()
    expect(theSwitch('settings.transmit.tciServer.cw.label')).toBeNull()
    expect(theSwitch('settings.transmit.tciServer.spots.label')).toBeNull()
    expect(container.querySelector('[data-tci-server-programs]')).toBeNull()
    cleanup()
    // CONTROL: switched on, the same queries find each control, so the absences above can fail.
    setup({ tciServer: true }, serving())
    expect(master().getAttribute('aria-checked')).toBe('true')
    expect(portBox()).not.toBeNull()
    expect(theSwitch('settings.transmit.tciServer.tx.label')).not.toBeNull()
    expect(theSwitch('settings.transmit.tciServer.cw.label')).not.toBeNull()
    expect(theSwitch('settings.transmit.tciServer.spots.label')).not.toBeNull()
    expect(document.querySelector('[data-tci-server-programs]')).not.toBeNull()
  })

  it('shows the address the station bound, not the port in the form, and Copy copies that one', () => {
    const writeText = vi.fn(() => Promise.resolve())
    Object.defineProperty(navigator, 'clipboard', { value: { writeText }, configurable: true })
    // A second Nexus window: the setting says 40001, the station serves on the next port.
    const { container } = setup({ tciServer: true, tciServerPort: 40001 }, serving({ port: 40002 }))
    expect(address()?.textContent).toBe('127.0.0.1:40002')
    expect(container.textContent).not.toContain('127.0.0.1:40001')
    fireEvent.click(screen.getByRole('button', { name: EN['settings.transmit.share.copy.action'] }))
    expect(writeText).toHaveBeenCalledWith('127.0.0.1:40002')
  })

  it("shows the station's sentence in the address's place when it could not bind the port", () => {
    const why =
      'Programs cannot connect by TCI: ports 40001 and 40002 on this computer are taken. Retrying quietly.'
    setup({ tciServer: true }, { port: null, error: why, programs: [] })
    expect(screen.getByRole('alert').textContent).toBe(why)
    expect(address()).toBeNull()
    expect(screen.queryByRole('button', { name: EN['settings.transmit.share.copy.action'] })).toBeNull()
  })

  it('says so while it is switched on and not listening yet, with no address', () => {
    setup({ tciServer: true }, undefined)
    expect(document.querySelector('[data-tci-server-waiting]')?.textContent).toBe(
      EN['settings.transmit.tciServer.waiting'],
    )
    expect(address()).toBeNull()
    cleanup()
    // CONTROL: listening, the line is gone and the address is there.
    setup({ tciServer: true }, serving())
    expect(document.querySelector('[data-tci-server-waiting]')).toBeNull()
    expect(address()?.textContent).toBe('127.0.0.1:40001')
  })

  it('saves each switch and the port through the form, and never a port outside 1–65535', () => {
    const { onSwitch, onPort } = setup({ tciServer: true }, serving())
    fireEvent.click(master())
    fireEvent.click(theSwitch('settings.transmit.tciServer.tx.label')!)
    fireEvent.click(theSwitch('settings.transmit.tciServer.cw.label')!)
    fireEvent.click(theSwitch('settings.transmit.tciServer.spots.label')!)
    expect(onSwitch.mock.calls).toEqual([
      ['tciServer', false],
      ['tciServerTx', true],
      ['tciServerCw', false],
      ['tciServerSpots', false],
    ])
    const port = portBox() as HTMLInputElement
    expect(port.value).toBe('40001')
    fireEvent.change(port, { target: { value: '40010' } })
    for (const value of ['0', '65536', '-1', '24.5', 'abc', '']) {
      fireEvent.change(port, { target: { value } })
    }
    expect(onPort.mock.calls).toEqual([[40010]])
  })

  it('shows the port Nexus serves on as it comes when the settings name none', () => {
    setup({ tciServer: true, tciServerPort: undefined }, serving())
    expect((portBox() as HTMLInputElement).value).toBe(String(TCI_SERVER_DEFAULT_PORT))
  })

  it('draws "Programs on TCI may transmit" only at the station', () => {
    setup({ tciServer: true }, serving())
    const here = theSwitch('settings.transmit.tciServer.tx.label')!
    expect((here as HTMLButtonElement).disabled).toBe(false)
    expect(here.getAttribute('aria-checked')).toBe('false')
    expect(here.getAttribute('data-setting')).toBe('tciServerTx')
    expect(document.querySelector('[data-set-at-station="tciServerTx"]')).toBeNull()
    cleanup()
    // From afar, on or off at the shack: no switch, and the row says where it is set.
    for (const on of [false, true]) {
      setup({ tciServer: true, tciServerTx: on }, serving(), FROM_AFAR)
      expect(theSwitch('settings.transmit.tciServer.tx.label')).toBeNull()
      expect(document.querySelector('[data-setting="tciServerTx"]')).toBeNull()
      const row = document.querySelector('[data-tci-server-tx]') as HTMLElement
      expect(row.textContent).toContain(EN['settings.transmit.tciServer.tx.label'])
      expect(row.querySelector('[data-set-at-station="tciServerTx"]')?.textContent).toBe(
        EN['settings.stationOnly.hint'],
      )
      // CONTROL: the station's other settings are drawn from afar, read-only, each saying so.
      for (const key of [
        'settings.transmit.tciServer.label',
        'settings.transmit.tciServer.cw.label',
        'settings.transmit.tciServer.spots.label',
      ] as const) {
        expect((theSwitch(key) as HTMLButtonElement).disabled, key).toBe(true)
      }
      expect((portBox() as HTMLInputElement).disabled).toBe(true)
      for (const key of ['tciServer', 'tciServerPort', 'tciServerCw', 'tciServerSpots']) {
        expect(document.querySelector(`[data-set-at-station="${key}"]`), key).not.toBeNull()
      }
      cleanup()
    }
  })

  it('says beside the transmit switch that slot-timed modes may not transmit through TCI yet', () => {
    for (const where of [AT_THE_STATION, FROM_AFAR]) {
      setup({ tciServer: true }, serving(), where)
      const row = document.querySelector('[data-tci-server-tx]') as HTMLElement
      expect(row.textContent).toContain(EN['settings.transmit.tciServer.tx.slotTimed'])
      cleanup()
    }
  })

  it("calls nothing not ready: a program's own audio, its spots and a Flex's DAX audio all work", () => {
    const { container } = setup({ tciServer: true }, serving())
    const section = container.querySelector('[data-tci-server]') as HTMLElement
    expect(section.textContent).not.toMatch(/not ready/i)
    // CONTROL: what is still refused is still said, beside the transmit switch.
    expect(section.textContent).toContain(EN['settings.transmit.tciServer.tx.slotTimed'])
  })

  it('lists the programs connected, what each is doing and its last refusal, and disconnects the one asked for', () => {
    const { onDisconnect } = setup({ tciServer: true }, serving({ programs: [WSJTX, LOGGER] }))
    const [first, second] = programRows()
    expect(programRows()).toHaveLength(2)
    expect(first.textContent).toContain(t('settings.transmit.tciServer.program.name', { id: 1 }))
    expect(first.textContent).toContain('127.0.0.1:53412')
    expect(first.textContent).toContain(EN['settings.transmit.tciServer.program.audio'])
    expect(first.textContent).not.toContain(EN['settings.transmit.tciServer.program.keyed'])
    expect(first.querySelector('[data-tci-program-refused]')?.textContent).toBe(
      t('settings.transmit.tciServer.program.refused', { why: 'programs on TCI may not transmit' }),
    )
    expect(second.textContent).toContain(t('settings.transmit.tciServer.program.name', { id: 2 }))
    expect(second.textContent).toContain('[::1]:53500')
    expect(second.textContent).toContain(EN['settings.transmit.tciServer.program.keyed'])
    expect(second.textContent).not.toContain(EN['settings.transmit.tciServer.program.audio'])
    expect(second.querySelector('[data-tci-program-refused]')).toBeNull()
    fireEvent.click(
      within(second).getByRole('button', { name: EN['settings.transmit.tciServer.disconnect.action'] }),
    )
    expect(onDisconnect.mock.calls).toEqual([[2]])
  })

  it("offers no Disconnect and no Copy from afar, where neither would act on the station's computer", () => {
    setup({ tciServer: true }, serving({ programs: [WSJTX] }), FROM_AFAR)
    // The program and the address are still shown, as the station reports them.
    expect(programRows()).toHaveLength(1)
    expect(programRows()[0].textContent).toContain('127.0.0.1:53412')
    expect(address()?.textContent).toBe('127.0.0.1:40001')
    expect(screen.queryByRole('button', { name: EN['settings.transmit.tciServer.disconnect.action'] })).toBeNull()
    expect(screen.queryByRole('button', { name: EN['settings.transmit.share.copy.action'] })).toBeNull()
    cleanup()
    // CONTROL: at the station both are there.
    setup({ tciServer: true }, serving({ programs: [WSJTX] }))
    expect(screen.queryByRole('button', { name: EN['settings.transmit.tciServer.disconnect.action'] })).not.toBeNull()
    expect(screen.queryByRole('button', { name: EN['settings.transmit.share.copy.action'] })).not.toBeNull()
  })

  it('says so when no program is connected', () => {
    setup({ tciServer: true }, serving())
    expect(programRows()).toHaveLength(0)
    expect(document.querySelector('[data-tci-server-programs]')?.textContent).toContain(
      EN['settings.transmit.tciServer.programs.none'],
    )
  })

  it("shows what the station reports: a key that ended, a program that left, a server switched off", () => {
    const props = { onSwitch: vi.fn(), onPort: vi.fn(), onDisconnect: vi.fn(), ...AT_THE_STATION }
    const { rerender } = render(
      <TciServerSettings form={form({ tciServer: true })} status={serving({ programs: [LOGGER] })} {...props} />,
    )
    expect(programRows()[0].textContent).toContain(EN['settings.transmit.tciServer.program.keyed'])
    // "Programs on TCI may transmit" turned off: the station ends the key, and says so.
    rerender(
      <TciServerSettings
        form={form({ tciServer: true })}
        status={serving({ programs: [{ ...LOGGER, keyed: false }] })}
        {...props}
      />,
    )
    expect(programRows()[0].textContent).not.toContain(EN['settings.transmit.tciServer.program.keyed'])
    // The server switched off: the station closes every connection and reports no server.
    rerender(<TciServerSettings form={form({ tciServer: false })} status={undefined} {...props} />)
    expect(programRows()).toHaveLength(0)
    expect(address()).toBeNull()
  })
})

describe('the CAT broker and the TCI server on one port', () => {
  it('is a clash only while both are on and ask for the same port', () => {
    expect(sharePortClash(form({ tciServer: true, tciServerPort: 4532 }))).toBe(4532)
    expect(sharePortClash(form({ tciServer: true, catBrokerPort: 40001 }))).toBe(40001)
    // The TCI port a settings file does not name is the one the server comes on.
    expect(sharePortClash(form({ tciServer: true, catBrokerPort: 40001, tciServerPort: undefined }))).toBe(40001)
    expect(sharePortClash(form({ tciServer: true }))).toBeNull()
    expect(sharePortClash(form({ tciServer: false, tciServerPort: 4532 }))).toBeNull()
    expect(sharePortClash(form({ tciServer: true, tciServerPort: 4532, catBroker: false }))).toBeNull()
    expect(sharePortClash(null)).toBeNull()
    expect(sharePortClash(undefined)).toBeNull()
  })
})

describe("the station's own numbers and words", () => {
  const read = (rel: string) => readFileSync(fileURLToPath(new URL(rel, import.meta.url)), 'utf8')

  it('comes on the port the station serves on as it comes', () => {
    const rust = /pub const TCI_SERVER_DEFAULT_PORT: u16 = (\d+);/.exec(
      read('../../../crates/tempo-app/src/settings.rs'),
    )?.[1]
    expect(rust, 'CONTROL: the constant is found in settings.rs').toBeTruthy()
    expect(TCI_SERVER_DEFAULT_PORT).toBe(Number(rust))
    expect(defaultSettings.tciServerPort).toBe(TCI_SERVER_DEFAULT_PORT)
  })

  it('reads every field of the status the station sends', () => {
    const dto = read('../../../crates/tempo-app/src/dto.rs')
    const types = read('../types.ts')
    const camel = (s: string) => s.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase())
    const rustFields = (src: string, name: string) => {
      const body = new RegExp(`pub struct ${name} \\{([\\s\\S]*?)\\n\\}`).exec(src)?.[1] ?? ''
      return [...body.matchAll(/^\s*pub (\w+):/gm)].map((m) => camel(m[1])).sort()
    }
    const tsFields = (name: string) => {
      const body = new RegExp(`export interface ${name} \\{([\\s\\S]*?)\\n\\}`).exec(types)?.[1] ?? ''
      return [...body.matchAll(/^ {2}(\w+)\??:/gm)].map((m) => m[1]).sort()
    }
    expect(tsFields('TciServerStatus')).toEqual(rustFields(dto, 'TciServerStatus'))
    expect(tsFields('TciProgramStatus')).toEqual(rustFields(dto, 'TciProgramStatus'))
    expect(rustFields(dto, 'TciProgramStatus')).toEqual(['audio', 'id', 'keyed', 'peer', 'refused'])
    // CONTROL: a field the station starts sending is seen, so the equality above can fail.
    const grown = dto.replace('pub struct TciProgramStatus {', 'pub struct TciProgramStatus {\n    pub tx_audio: bool,')
    expect(rustFields(grown, 'TciProgramStatus')).toContain('txAudio')
  })

  it("gives the pt-BR kit a row for each of its strings, with the English the app shows", () => {
    const csv = read('../../../translations/pt-BR/nexus-ptbr-translation.csv')
    const keys = [
      ...Object.keys(EN).filter((k) => k.startsWith('settings.transmit.tciServer.')),
      'settings.save.tciServerPortClash',
    ] as (keyof typeof EN)[]
    expect(keys.length, 'CONTROL: the strings are found').toBeGreaterThan(20)
    const missing = keys.filter((k) => !csv.includes(`,"${k}","${String(EN[k]).replace(/"/g, '""')}"`))
    expect(missing).toEqual([])
  })
})

describe('the settings search', () => {
  it('finds Share by TCI by its name and by the programs that connect to it', () => {
    for (const q of ['share by tci', 'jtdx', 'mshv', 'log4om', 'tci client rx1']) {
      expect(searchSettings(q).map((h) => h.section.id), q).toContain('transmit-limits')
    }
    expect(searchSettings('share by tci')[0]?.section.id).toBe('transmit-limits')
    // CONTROL: plain "tci" still lands first on Rig & CAT, where an SDR program's TCI server is.
    expect(searchSettings('tci')[0]?.section.id).toBe('rig-control')
  })
})
