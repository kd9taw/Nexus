// @vitest-environment jsdom
//
// THE FIRST-RUN JOURNEYS. The first step (About you) branches: switching from another program
// (Experienced) brings the old log in through the import preview and shows where contacts go, with
// QRZ Logbook's Test; two or more radios hand the rig step on to the radio setup once the second
// radio is saved. Offered and never forced: an untouched wizard passes nothing to write (the licence
// opens on the class the station holds), and nothing in it uploads a contact or takes a login.
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest'
import { render, screen, cleanup, fireEvent, waitFor, within } from '@testing-library/react'
import { SetupWizard } from './SetupWizard'
import { EN } from '../i18n'

vi.setConfig({ testTimeout: 15_000 })

const api = vi.hoisted(() => {
  const spies: Record<string, ReturnType<typeof vi.fn>> = {}
  const get = (name: string) => {
    if (!spies[name]) spies[name] = vi.fn(() => Promise.resolve(null))
    return spies[name]
  }
  return { spies, get }
})
vi.mock('../api', async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>()
  const mod: Record<string, unknown> = {}
  for (const name of Object.keys(actual)) {
    mod[name] = typeof actual[name] === 'function' ? api.get(name) : actual[name]
  }
  return mod
})

const RADIO = { enabled: true, baud: 38400, rigConn: 'serial', rigAddr: '', bands: [], pttMethod: 'vox', pttSerialPort: '' }
let station: Record<string, unknown> = {}

const PREVIEW = {
  format: 'adif',
  template: null,
  records: 3,
  contacts: 3,
  firstUnix: 1_600_000_000,
  lastUnix: 1_700_000_000,
  new: 3,
  duplicates: 0,
  updates: 0,
  notes: [],
  coverage: [{ field: 'CALL', count: 3 }],
  unmapped: [],
}
const DONE = {
  stats: { added: 3, skipped: 0, updated: 0, total: 3, run: 'run-1' },
  report: { ...PREVIEW, added: 3, duplicates: 0, updated: 0, newOnly: false, fields: [], accounted: true },
}
const CREDS = [
  // Lookup only: nothing goes there, so it is not a place contacts go.
  { id: 'qrz-xml', connector: 'QRZ callbook (name/QTH)', stored: true, identity: 'W1AW', uploads: false, enabled: false, lastSuccessUnix: null, lastFailureUnix: null, lastFailureDetail: null, paused: false },
  { id: 'qrz-logbook', connector: 'QRZ Logbook', stored: true, identity: 'W1AW', uploads: true, enabled: true, lastSuccessUnix: null, lastFailureUnix: null, lastFailureDetail: null, paused: false },
  { id: 'lotw', connector: 'LoTW', stored: false, identity: '', uploads: true, enabled: false, lastSuccessUnix: null, lastFailureUnix: null, lastFailureDetail: null, paused: false },
]

beforeEach(() => {
  for (const spy of Object.values(api.spies)) {
    spy.mockReset()
    spy.mockImplementation(() => Promise.resolve(null))
  }
  station = {
    mycall: 'W1AW',
    mygrid: 'FN31pr',
    licenseClass: 'technician',
    activeRadio: 0,
    audioIn: '',
    audioOut: '',
    pttMethod: 'vox',
    radios: [{ ...RADIO, id: 0, name: 'FTDX10', serialPort: 'COM5', rigModel: 1042, audioIn: '', audioOut: '' }],
    routingRules: [],
    defaultRadio: null,
    radioSwitchPolicy: 'act',
  }
  api.get('getSettings').mockImplementation(() => Promise.resolve(structuredClone(station)))
  api.get('addRadio').mockImplementation(() => {
    const radios = station.radios as Record<string, unknown>[]
    station = { ...station, radios: [...radios, { ...RADIO, id: 1, name: 'FT-991', serialPort: '', rigModel: 0, audioIn: '', audioOut: '' }] }
    return Promise.resolve(null)
  })
  api.get('getAudioDevices').mockImplementation(() => Promise.resolve({ input: [], output: [] }))
  api.get('getRigModels').mockImplementation(() => Promise.resolve([]))
  api.get('discoverFlex').mockImplementation(() => Promise.resolve([]))
  api.get('tciProbe').mockImplementation(() => Promise.resolve({ host: '127.0.0.1', resolved: true, tried: [], found: null }))
  api.get('detectRigs').mockImplementation(() =>
    Promise.resolve([
      { portName: 'COM5', suggestedModel: 1042, suggestedModelName: 'Yaesu FTDX10', suggestedAudio: null, suggestedAudioOut: null, chip: 'CP210x', civSide: null, interfaceName: null, interfacePttMethod: null },
    ]),
  )
  api.get('probeCatPorts').mockImplementation(() =>
    Promise.resolve({ found: true, portName: 'COM7', baud: 38400, model: 1035, modelName: 'Yaesu FT-991', freqMhz: 7.074, detail: 'FT-991 on COM7', modelSeeded: false }),
  )
  api.get('getCredentialsStatus').mockImplementation(() => Promise.resolve(structuredClone(CREDS)))
  api.get('qrzTestConnection').mockImplementation(() => Promise.resolve('OK, 1234 QSOs'))
  api.get('previewImport').mockImplementation(() => Promise.resolve(structuredClone(PREVIEW)))
  api.get('importLog').mockImplementation(() => Promise.resolve(structuredClone(DONE)))
})
afterEach(() => cleanup())

function wizard(extra: { askExperience?: boolean; rerun?: boolean } = {}) {
  const props = {
    settings: structuredClone(station) as never,
    onApply: vi.fn(),
    onTestCat: vi.fn() as never,
    onSkip: vi.fn(),
    onOpenServices: vi.fn(),
    askExperience: true,
    ...extra,
  }
  render(<SetupWizard {...props} />)
  return props
}
const answer = (name: string) => fireEvent.click(screen.getByRole('button', { name: new RegExp(`^${name}`) }))
const next = () => fireEvent.click(screen.getByRole('button', { name: /^Next/ }))
const finish = () => fireEvent.click(screen.getByRole('button', { name: /^Finish/ }))
/** About you → Station → Rig → Log. */
async function toLogStep() {
  next()
  next()
  await screen.findByRole('heading', { name: EN['setup.rig.title'] })
  next()
  await screen.findByRole('heading', { name: EN['setup.log.title'] })
}

describe('the licence step writes only a change', () => {
  it('opens on the class the station holds; an untouched Finish passes no class to write', async () => {
    const { onApply } = wizard({ askExperience: false, rerun: true })
    for (let i = 0; i < 3; i++) next()
    const tech = screen.getByRole('button', { name: new RegExp(`^${EN['setup.finish.license.technician']}`) })
    expect(tech.getAttribute('aria-pressed')).toBe('true')
    finish()
    expect(onApply).toHaveBeenCalledTimes(1)
    expect(onApply.mock.calls[0][3]).toBeNull()
    expect(onApply.mock.calls[0][4]).toEqual({})
  })

  it('a different class picked is the class passed', async () => {
    const { onApply } = wizard({ askExperience: false, rerun: true })
    for (let i = 0; i < 3; i++) next()
    fireEvent.click(screen.getByRole('button', { name: new RegExp(`^${EN['setup.finish.license.general']}`) }))
    finish()
    expect(onApply.mock.calls[0][3]).toBe('general')
  })
})

describe('switching apps: the log through the preview, and where contacts go', () => {
  it('Experienced lists the services contacts go to, and QRZ Test asks QRZ for its status', async () => {
    wizard()
    answer(EN['setup.about.experienced'])
    await toLogStep()
    const panel = (await screen.findByRole('heading', { name: EN['setup.services.title'] })).parentElement as HTMLElement
    await within(panel).findByText('QRZ Logbook')
    const names = [...panel.querySelectorAll('.conn-name')].map((n) => n.textContent)
    expect(names).toEqual(['QRZ Logbook', 'LoTW'])
    fireEvent.click(within(panel).getByRole('button', { name: EN['settings.connections.qrz.test.action'] }))
    expect(await within(panel).findByText('✓ QRZ Logbook reachable: OK, 1234 QSOs')).toBeTruthy()
    expect(api.spies.qrzTestConnection).toHaveBeenCalledTimes(1)
  })

  it('control: New to the hobby gets the log step without the services', async () => {
    wizard()
    answer(EN['setup.about.newToHobby'])
    await toLogStep()
    expect(screen.queryByRole('heading', { name: EN['setup.services.title'] })).toBeNull()
    expect(api.spies.getCredentialsStatus).not.toHaveBeenCalled()
  })

  it('a picked log opens the preview; nothing is imported until Import is pressed there', async () => {
    wizard()
    answer(EN['setup.about.experienced'])
    await toLogStep()
    const input = document.querySelector('.wizard-log-import input[type=file]') as HTMLInputElement
    const file = new File(['<CALL:4>K1AB<EOR>'], 'old.adi', { type: 'text/plain' })
    fireEvent.change(input, { target: { files: [file] } })
    const preview = await screen.findByRole('dialog', { name: EN['logbook.import.preview.title'] })
    await within(preview).findByText('3 new contacts')
    expect(api.spies.previewImport).toHaveBeenCalledWith('<CALL:4>K1AB<EOR>')
    expect(api.spies.importLog).not.toHaveBeenCalled()
    expect(api.spies.importAdif).not.toHaveBeenCalled()
    fireEvent.click(within(preview).getByRole('button', { name: EN['logbook.import.preview.import'] }))
    await waitFor(() => expect(api.spies.importLog).toHaveBeenCalledWith('<CALL:4>K1AB<EOR>', false))
    fireEvent.click(await within(preview).findByRole('button', { name: EN['logbook.import.done.close'] }))
    await waitFor(() => expect(screen.queryByRole('dialog', { name: EN['logbook.import.preview.title'] })).toBeNull())
    expect(document.querySelector('.wizard-log-result')?.textContent).toMatch(/Imported 3 QSOs/)
  })

  it('Cancel in the preview imports nothing', async () => {
    wizard()
    answer(EN['setup.about.experienced'])
    await toLogStep()
    const input = document.querySelector('.wizard-log-import input[type=file]') as HTMLInputElement
    fireEvent.change(input, { target: { files: [new File(['x'], 'old.adi')] } })
    const preview = await screen.findByRole('dialog', { name: EN['logbook.import.preview.title'] })
    fireEvent.click(await within(preview).findByRole('button', { name: EN['logbook.import.preview.cancel'] }))
    await waitFor(() => expect(screen.queryByRole('dialog', { name: EN['logbook.import.preview.title'] })).toBeNull())
    expect(api.spies.importLog).not.toHaveBeenCalled()
    expect(document.querySelector('.wizard-log-result')).toBeNull()
  })

  it('Settings ▸ Confirmations opens after Finish only when asked for there', async () => {
    const asked = wizard()
    answer(EN['setup.about.experienced'])
    await toLogStep()
    const offer = await screen.findByRole('button', { name: new RegExp(`^${EN['setup.services.openAfter.label']}`) })
    expect(offer.getAttribute('aria-pressed')).toBe('false')
    fireEvent.click(offer)
    next()
    finish()
    expect(asked.onOpenServices).toHaveBeenCalledTimes(1)
    cleanup()
    // Control: not asked for, not opened.
    const notAsked = wizard()
    answer(EN['setup.about.experienced'])
    await toLogStep()
    await screen.findByRole('button', { name: new RegExp(`^${EN['setup.services.openAfter.label']}`) })
    next()
    finish()
    expect(notAsked.onOpenServices).not.toHaveBeenCalled()
  })
})

describe('two or more radios: the rig step hands on to the radio setup', () => {
  async function addSecond(count: string | null) {
    wizard()
    if (count) answer(count)
    next()
    next()
    fireEvent.click(await screen.findByRole('button', { name: /FTDX10/ }))
    fireEvent.click(await screen.findByRole('button', { name: /second radio/ }))
    await screen.findByText(/FT-991 on COM7|Yaesu FT-991/)
  }
  it('Two or more: once the second radio is saved, the radio setup opens', async () => {
    await addSecond(EN['setup.about.radios.several'])
    expect(await screen.findByRole('dialog', { name: 'Set up radios' })).toBeTruthy()
  })
  it('control: one radio, or no answer, and the setup waits for its button', async () => {
    await addSecond(EN['setup.about.radios.one'])
    await new Promise((r) => setTimeout(r, 50))
    expect(screen.queryByRole('dialog', { name: 'Set up radios' })).toBeNull()
    expect(screen.getByRole('button', { name: /Set up which radio does what/ })).toBeTruthy()
    cleanup()
    await addSecond(null)
    await new Promise((r) => setTimeout(r, 50))
    expect(screen.queryByRole('dialog', { name: 'Set up radios' })).toBeNull()
  })
})

// Every api verb that sends a contact out, fetches from a logbook service, or stores a login.
const SENDS_OR_LOGS_IN =
  /push|upload|^sync(Qrz|Lotw|Clublog|Eqsl)|^download(Lotw|Eqsl)|^set\w*(Password|Key|Code|Token)$|^setUploadToggle|^markLotwUploaded/i

describe('nothing uploads from the wizard', () => {
  it('the whole switching-apps journey (import, QRZ Test, Finish) calls no upload and stores no login', async () => {
    // The net covers the api as it is: these must all be caught by it.
    const verbs = Object.keys(api.spies)
    for (const v of ['qrzPushQso', 'clublogPushQso', 'eqslPushQso', 'hrdlogPushQso', 'uploadLotwReport', 'syncQrz', 'downloadLotwReport', 'setLotwPassword', 'setQrzLogbookKey', 'setClublogPassword', 'setEqslPassword']) {
      expect(verbs).toContain(v)
      expect(SENDS_OR_LOGS_IN.test(v)).toBe(true)
    }
    const { onOpenServices, onApply } = wizard()
    answer(EN['setup.about.experienced'])
    await toLogStep()
    const input = document.querySelector('.wizard-log-import input[type=file]') as HTMLInputElement
    fireEvent.change(input, { target: { files: [new File(['<CALL:4>K1AB<EOR>'], 'old.adi')] } })
    const preview = await screen.findByRole('dialog', { name: EN['logbook.import.preview.title'] })
    fireEvent.click(await within(preview).findByRole('button', { name: EN['logbook.import.preview.import'] }))
    fireEvent.click(await within(preview).findByRole('button', { name: EN['logbook.import.done.close'] }))
    fireEvent.click(await screen.findByRole('button', { name: EN['settings.connections.qrz.test.action'] }))
    await screen.findByText(/QRZ Logbook reachable/)
    fireEvent.click(screen.getByRole('button', { name: new RegExp(`^${EN['setup.services.openAfter.label']}`) }))
    next()
    finish()
    expect(onApply).toHaveBeenCalledTimes(1)
    expect(onOpenServices).toHaveBeenCalledTimes(1)
    const called = verbs.filter((v) => api.spies[v].mock.calls.length > 0)
    expect(called.filter((v) => SENDS_OR_LOGS_IN.test(v))).toEqual([])
    // What it did call, by name: reads, the import the operator pressed, and the one status test.
    expect(called).toContain('importLog')
    expect(called).toContain('qrzTestConnection')
  })
})
