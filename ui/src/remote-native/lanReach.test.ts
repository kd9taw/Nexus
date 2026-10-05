// The other computer's sentences for a station it could not reach or find, one per condition.
import { describe, expect, it } from 'vitest'
import { lanFindLine, lanReachLine, lanTypedLine, type LanUnreached } from './lanReach'

describe('why the station could not be reached, said plainly', () => {
  it('names the condition each code stands for, and no two alike', () => {
    const said: [LanUnreached, RegExp][] = [
      ['otherNetwork', /^That address isn’t on this computer’s network\./],
      ['refused', /nothing listens at that port: Remote over this network is off there, or on another port/],
      ['noAnswer', /^No answer from the station\./],
    ]
    for (const [code, sentence] of said) expect(lanReachLine(code), code).toMatch(sentence)
    expect(new Set(said.map(([code]) => lanReachLine(code))).size).toBe(said.length)
  })

  it('with no answer, names each thing that looks the same from here and blames none', () => {
    const line = lanReachLine('noAnswer')
    for (const cause of [/off there/, /firewall may be blocking Nexus \(allow it on Private networks at the station\)/,
      /keep its devices apart \(guest Wi-Fi often does\)/, /may be asleep/]) expect(line).toMatch(cause)
    expect(line).toMatch(/Nexus can’t tell which from here\.$/)
    // CONTROL: a refusal is a different answer, and does not hedge.
    expect(lanReachLine('refused')).not.toMatch(/can’t tell/)
  })

  it('falls back to the typed address when nothing is found by name, for either reason', () => {
    expect(lanFindLine(false)).toBe('Finding stations by name doesn’t work on this computer. Type the address the station shows.')
    expect(lanFindLine(true)).toBe('No station answered by name on this network. Type the address the station shows.')
    expect(lanTypedLine()).toMatch(/192\.168\.1\.20 or 192\.168\.1\.20:42075/)
  })
})
