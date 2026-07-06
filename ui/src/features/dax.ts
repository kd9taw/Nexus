// SmartSDR DAX virtual-audio detection — shared by the Settings network-rig
// section and the setup wizard's rig step (one matcher, no copy-paste drift).
// DAX devices are plain Windows sound devices ("DAX Audio RX 1", "DAX Audio TX");
// when both sides exist, one click pairs them as Nexus's audio in/out.

export interface DaxPair {
  input: string
  output: string
}

/** Find the DAX receive/transmit devices in the system device lists, preferring
 * RX 1 (the slice-A stream) when several DAX RX channels exist. Null when
 * either side is missing — the pairing affordance simply doesn't render. */
export function findDaxDevices(input: string[], output: string[]): DaxPair | null {
  const rx = input.find((d) => /dax.*rx\s*1/i.test(d)) ?? input.find((d) => /dax/i.test(d))
  const tx = output.find((d) => /dax.*tx/i.test(d)) ?? output.find((d) => /dax/i.test(d))
  return rx && tx ? { input: rx, output: tx } : null
}
