// A saved audio device that is no longer offered, and the one offered device it most likely is now
// (#425). Windows renamed a radio's "Speakers (USB AUDIO CODEC)" to "Speakers (2- USB AUDIO  CODEC)"
// after a USB change: a "2- " number in front, and two spaces before CODEC in the device's real
// name. The saved name matched neither, and the picker showed it as "saved, not in the list" right
// beside the working entry, the two looking the same.
//
// The backend says the same in its open error (crates/tempo-audio/src/device.rs,
// `renumbered_twin`); the rule here is that one's, kept beside it.
//
// ⚠️ AN OFFER, NEVER A SWITCH. The picker shows a button the operator presses, and Save still
// decides. Transmit audio sent to the wrong device is worse than none.

/** `name` without the "N- " Windows puts before a second identically named device's name, at the
 *  start or just inside a parenthesis ("Speakers (2- USB Audio CODEC)" is "Speakers (USB Audio
 *  CODEC)"). The backend's `without_instance_numbers` (crates/tempo-audio/src/notify_sound.rs). */
function withoutInstanceNumbers(name: string): string {
  return name.replace(/(^|\()\d+- /g, '$1')
}

/** The name with every run of whitespace one space and none at the ends, then without the number. */
function looseKey(name: string): string {
  return withoutInstanceNumbers(name.split(/\s+/).filter(Boolean).join(' '))
}

/** `name` split into its base and the " #N" (N ≥ 2) a device list adds to the second and later of
 *  identically named devices; a name without one is the first. The backend's `split_device_ordinal`
 *  (crates/tempo-audio/src/audiodev.rs). */
function splitOrdinal(name: string): [string, number] {
  const at = name.lastIndexOf(' #')
  if (at >= 0) {
    const rest = name.slice(at + 2)
    const n = /^\+?\d+$/.test(rest) ? Number(rest) : NaN
    if (n >= 2) return [name.slice(0, at), n]
  }
  return [name, 1]
}

/** The ONE offered device that is `saved` but for the Windows number and the spacing, or null when
 *  none is, or when it is not the only one: two such devices, or one of two identically named
 *  codecs (the list names the second "… #2", the backend's raw list names both alike, and which
 *  radio's codec is which is not for the picker to guess). The saved name's own " #N" still
 *  counts, so a second codec's saved name never points at the first. `taken` is what the station's
 *  other radios use in the same direction: never offered, so a two-radio station is never pointed
 *  at the other radio's codec. Null as well when `saved` is offered as it is. */
export function renumberedTwin(
  saved: string,
  offered: readonly string[],
  taken: readonly string[] = [],
): string | null {
  if (!saved.trim() || offered.includes(saved)) return null
  const want = looseKey(saved)
  const twins = offered.filter((d) => looseKey(splitOrdinal(d)[0]) === want)
  if (twins.length !== 1 || splitOrdinal(twins[0])[1] !== 1) return null
  return taken.includes(twins[0]) ? null : twins[0]
}

/** `name` with the whitespace a reader cannot see shown as ␣: at its ends, and every run of more
 *  than one space or of anything but a space. A single space between words stays a space, so an
 *  ordinary name reads as it always did. */
export function showHiddenSpaces(name: string): string {
  return name.replace(/\s+/g, (run: string, at: number) =>
    run === ' ' && at > 0 && at + run.length < name.length ? run : '␣'.repeat(run.length),
  )
}
