//! Which rig does this sound card belong to? Answered from USB topology, on macOS.
//!
//! # The problem
//!
//! Two rig interfaces can report the byte-identical CoreAudio device name. An ON8ST station
//! (2026-08-13) runs an FT-710 and an FTX-1 whose codecs are both C-Media, both called
//! `USB Audio Device`, differing only in channel count. [`crate::audiodev::disambiguate_names`]
//! keeps them selectable by appending `" #2"`, but that ordinal is positional and says nothing
//! about WHICH radio it is — the operator picked between two identical strings by trial and
//! error, and a wrong guess sends TX audio to the other rig.
//!
//! Nothing in the devices themselves helps: a C-Media codec carries no serial number, and its
//! name is fixed in the chip. The only fact that distinguishes them is WHERE THEY ARE PLUGGED IN.
//!
//! What this module currently does about it is narrower than that framing suggests, deliberately:
//! it does not rename anything. It hands the answer over as STRUCTURED FACTS, and the pickers keep
//! showing exactly the strings they showed before — so a wrong pick is now DETECTED and said out
//! loud at save time, rather than prevented at pick time. Prevention means rewriting displayed
//! labels, which is the follow-up named at the end of this header.
//!
//! # The mechanism
//!
//! macOS gives every USB device a `locationID`: a hex port path whose leading byte is the
//! controller and whose following nibbles are the port used at each hub tier, zero-padded right.
//! A rig that presents CAT and audio over one cable is internally a hub, so its interfaces are
//! SIBLINGS — their paths differ only in the final nibble:
//!
//! ```text
//!   FT-710 CAT   (CP2105)  0x111000  ┐ parent 0x110000
//!   FT-710 audio (C-Media) 0x112000  ┘
//!   FTX-1  CAT   (CP2105)  0x121000  ┐ parent 0x120000
//!   FTX-1  audio (C-Media) 0x122000  ┘
//! ```
//!
//! So: take the codec's location, take the location of the CAT port each radio profile is
//! configured on, reduce both to their parent hub ([`parent_hub`]), and a match names the radio.
//! Both numbers are obtainable:
//!
//! * **Audio** — free, no IOKit. CoreAudio's `kAudioDevicePropertyDeviceUID` for a USB device
//!   embeds it: `AppleUSBAudioEngine:C-Media Electronics Inc.:USB Audio Device:112000:2,1`.
//! * **Serial** — one IOKit call. `IORegistryEntrySearchCFProperty` with
//!   `kIORegistryIterateParents` walks up from the `IOSerialBSDClient` node to the USB device
//!   that owns it, because the tty lives in the IOService plane, not the IOUSB one.
//!
//! # What this is not
//!
//! The nibble-per-tier encoding is a long-standing Apple convention, not a documented contract,
//! so this is a well-founded heuristic rather than a guarantee — every function here fails soft,
//! returning `None`/empty so the picker simply falls back to today's labels. And a `locationID`
//! describes a PHYSICAL PORT: replug a rig into a different socket and it changes. That is the
//! right behaviour (the label follows the wiring) but it means the mapping must be recomputed at
//! every enumeration and MUST NEVER be persisted — settings continue to store the device name.
//!
//! And it is a TIE-BREAK, never a replacement. Every consumer below runs the existing string rule
//! FIRST and asks topology only about what the string could not settle. That ordering is not
//! politeness: string matching works on every platform and has years of field evidence behind it,
//! while this reads an undocumented convention on one OS. Where the two could disagree, the string
//! wins.
//!
//! # Where this is used, and what each caller may not do with it
//!
//! * [`crate::ports`] — collapses the duplicate serial rows a NAME cannot pair (the same bridge
//!   offered once by Apple's driver as `cu.usbserial-…` and again by the vendor's as `cu.SLAB_…`).
//!   Runs behind the name-based collapse and DROPS NOTHING it cannot key, because losing a real
//!   port looks exactly like a rig that stopped existing.
//! * `get_serial_ports_detailed` / `get_audio_devices` (src-tauri) — annotate each row with the
//!   interface index, how many interfaces that USB device has in total, the paired sound card, and
//!   the parent hub. Every one is an `Option`, every one is `None` off macOS, and the name and
//!   label they sit beside are unchanged.
//!
//!   Note which of the two relations each uses, because they are NOT equally strong. Two serial
//!   interfaces of one bridge share the EXACT SAME `locationID`, so counting them is exact. A
//!   rig's CAT bridge and its codec are separate USB devices behind the rig's internal hub, so
//!   they can only be related by [`parent_hub`] — and two unrelated things in one EXTERNAL hub
//!   share a parent too. That asymmetry is why the interface advice can be precise and the
//!   paired-audio reading can only ever raise a doubt.
//! * `checkRigForm` (ui) — two pre-save DIAGNOSTICS: the half of a dual bridge that carries no
//!   CAT, and a codec that is inside the other radio. Both are warnings and neither may ever
//!   refuse a save; a heuristic must not be able to lock an operator out of their own station.
//!
//! What is deliberately NOT here yet: rewriting the DISPLAYED label, so the audio picker reads
//! "USB Audio Device (FT-710)" instead of a bare `" #2"`. That is the operator-visible payoff and
//! it is a separate change — it alters what every picker shows and needs its own review of what
//! happens when the topology is wrong. Everything here only ADDS structured facts beside the
//! existing name and label; nothing displayed changes.
//!
//! # What CI can and cannot check here
//!
//! CI has no Mac with two radios on it, so the IOKit half is compile-checked and nothing more.
//! Everything that DECIDES anything is therefore a pure function taking the maps as parameters —
//! [`parent_hub`] and [`location_from_audio_uid`], plus `ports::collapse_usb_siblings` and
//! `checkRigForm` on the two sides that consume them — and their tests drive them with locations
//! measured on real hardware (the table above). What remains unproven by CI is only whether the
//! registry walk returns those numbers, which is why the walk itself decides nothing.

/// The parent hub of a USB port path: this path with its last non-zero nibble cleared.
///
/// Sibling interfaces of one composite device (a rig's CAT bridge and its codec) differ only in
/// that nibble, so equal parents means "same physical device". Returns `loc` unchanged when there
/// is no non-zero nibble — a root-port device has no hub to share, and comparing it to itself is
/// the correct degenerate answer.
pub fn parent_hub(loc: u32) -> u32 {
    for shift in (0..32).step_by(4) {
        if (loc >> shift) & 0xf != 0 {
            return loc & !(0xf << shift);
        }
    }
    loc
}

/// Pull the `locationID` out of a CoreAudio device UID, if it carries one.
///
/// USB audio UIDs look like
/// `AppleUSBAudioEngine:C-Media Electronics Inc.:USB Audio Device:112000:2,1` — the fourth
/// colon-separated field is the location in hex. Built-in and virtual devices (`BuiltInSpeaker`,
/// aggregate devices, Teams/OBS virtual cards) have no such field and yield `None`, which is
/// exactly right: they belong to no rig.
///
/// The manufacturer field is vendor text and may itself contain colons, so this reads the
/// location as "the last field that parses as hex and is followed by the channel-layout field"
/// rather than by a fixed index: split on ':', then take the LAST-BUT-ONE component.
pub fn location_from_audio_uid(uid: &str) -> Option<u32> {
    let parts: Vec<&str> = uid.split(':').collect();
    // `<...>:<location>:<channel layout>` — need at least a name, a location and a layout.
    if parts.len() < 3 {
        return None;
    }
    let candidate = parts[parts.len() - 2];
    // A bare decimal like "2" would parse as hex too, so require the shape of a port path:
    // non-empty, all hex digits, and more than one digit (real paths are 0x11000-scale).
    if candidate.len() < 2 || !candidate.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    u32::from_str_radix(candidate, 16).ok()
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use core_foundation_sys::base::{kCFAllocatorDefault, CFRelease, CFTypeRef};
    use core_foundation_sys::number::{kCFNumberSInt32Type, CFNumberGetValue, CFNumberRef};
    use core_foundation_sys::string::{
        kCFStringEncodingUTF8, CFStringCreateWithCString, CFStringGetCString, CFStringRef,
    };
    use coreaudio_sys::*;
    use io_kit_sys::keys::kIOServicePlane;
    use io_kit_sys::types::{io_iterator_t, io_object_t};
    use io_kit_sys::*;
    use std::collections::HashMap;
    use std::ffi::{CStr, CString};
    use std::ptr;

    /// A `CFStringRef` for `s`. Caller releases. `None` only if `s` contains a NUL, which no
    /// caller here can produce (all keys are literals).
    unsafe fn cfstr(s: &str) -> Option<CFStringRef> {
        let c = CString::new(s).ok()?;
        let r = CFStringCreateWithCString(kCFAllocatorDefault, c.as_ptr(), kCFStringEncodingUTF8);
        if r.is_null() {
            None
        } else {
            Some(r)
        }
    }

    /// Read a `CFStringRef` out into an owned `String`. 512 bytes is generous for the two keys
    /// this module reads (a device UID and a `/dev` path); a longer value is skipped rather than
    /// truncated, because a half-parsed UID would yield a wrong location, not a missing one.
    unsafe fn cfstring_to_string(v: CFStringRef) -> Option<String> {
        let mut buf = [0i8; 512];
        if CFStringGetCString(
            v,
            buf.as_mut_ptr(),
            buf.len() as isize,
            kCFStringEncodingUTF8,
        ) == 0
        {
            return None;
        }
        Some(CStr::from_ptr(buf.as_ptr()).to_string_lossy().into_owned())
    }

    /// `IORegistryEntryCreateCFProperty` for a string-valued key on one registry entry.
    unsafe fn registry_string(entry: io_object_t, key: &str) -> Option<String> {
        let k = cfstr(key)?;
        let v = IORegistryEntryCreateCFProperty(entry, k, kCFAllocatorDefault, 0);
        CFRelease(k as CFTypeRef);
        if v.is_null() {
            return None;
        }
        let out = cfstring_to_string(v as CFStringRef);
        CFRelease(v);
        out
    }

    /// The `locationID` of the nearest USB ancestor of `entry`.
    ///
    /// One call rather than a hand-rolled parent loop: `IORegistryEntrySearchCFProperty` with
    /// `kIORegistryIterateParents | kIORegistryIterateRecursively` climbs the IOService plane
    /// itself. The walk is necessary because a serial port's `IOSerialBSDClient` node carries the
    /// tty name but no location — the location belongs to the USB device several levels up.
    unsafe fn ancestor_location(entry: io_object_t) -> Option<u32> {
        ancestor_u32(entry, "locationID")
    }

    /// A `u32`-valued property from `entry` or the nearest ancestor that has it.
    ///
    /// One call rather than a hand-rolled parent loop: `IORegistryEntrySearchCFProperty` with
    /// `kIORegistryIterateParents | kIORegistryIterateRecursively` climbs the IOService plane
    /// itself. The walk is necessary because a serial port's `IOSerialBSDClient` node carries the
    /// tty name but neither the location nor the interface number — those belong to the USB
    /// interface and device several levels up.
    unsafe fn ancestor_u32(entry: io_object_t, key: &str) -> Option<u32> {
        let k = cfstr(key)?;
        let v = IORegistryEntrySearchCFProperty(
            entry,
            kIOServicePlane as *const std::os::raw::c_char,
            k,
            kCFAllocatorDefault,
            kIORegistryIterateRecursively | kIORegistryIterateParents,
        );
        CFRelease(k as CFTypeRef);
        if v.is_null() {
            return None;
        }
        let mut out: i32 = 0;
        let ok = CFNumberGetValue(
            v as CFNumberRef,
            kCFNumberSInt32Type,
            &mut out as *mut _ as *mut _,
        );
        CFRelease(v);
        if ok {
            Some(out as u32)
        } else {
            None
        }
    }

    /// Every USB serial port's `/dev/cu.*` callout path mapped to its USB location.
    ///
    /// Both driver nodes of a dual-claimed port appear (a CP2105 with the Silicon Labs extension
    /// installed alongside Apple's own driver yields `usbserial-*` AND `SLAB_USBtoUART*`); they
    /// report the SAME location, since they are two names for one physical interface, so either
    /// spelling in a radio profile resolves identically.
    pub fn serial_locations() -> HashMap<String, u32> {
        let mut out = HashMap::new();
        unsafe {
            let matching = IOServiceMatching(c"IOSerialBSDClient".as_ptr());
            if matching.is_null() {
                return out;
            }
            let mut it: io_iterator_t = 0;
            // Consumes `matching` whether it succeeds or fails — no leak on the error path.
            if IOServiceGetMatchingServices(kIOMasterPortDefault, matching, &mut it) != 0 {
                return out;
            }
            loop {
                let entry = IOIteratorNext(it);
                if entry == 0 {
                    break;
                }
                if let Some(tty) = registry_string(entry, "IOCalloutDevice") {
                    if let Some(loc) = ancestor_location(entry) {
                        out.insert(tty, loc);
                    }
                }
                IOObjectRelease(entry);
            }
            IOObjectRelease(it);
        }
        out
    }

    /// Each USB serial port's `/dev/cu.*` path mapped to its `bInterfaceNumber`.
    ///
    /// Distinguishes the halves of a multi-interface bridge: a CP2105 exposes two, and only
    /// interface 0 carries CAT on a Yaesu. Same registry walk as [`serial_locations`] — the
    /// number lives on the `IOUSBHostInterface` several levels above the tty node.
    pub fn serial_interfaces() -> HashMap<String, u32> {
        let mut out = HashMap::new();
        unsafe {
            let matching = IOServiceMatching(c"IOSerialBSDClient".as_ptr());
            if matching.is_null() {
                return out;
            }
            let mut it: io_iterator_t = 0;
            if IOServiceGetMatchingServices(kIOMasterPortDefault, matching, &mut it) != 0 {
                return out;
            }
            loop {
                let entry = IOIteratorNext(it);
                if entry == 0 {
                    break;
                }
                if let Some(tty) = registry_string(entry, "IOCalloutDevice") {
                    if let Some(n) = ancestor_u32(entry, "bInterfaceNumber") {
                        out.insert(tty, n);
                    }
                }
                IOObjectRelease(entry);
            }
            IOObjectRelease(it);
        }
        out
    }

    unsafe fn global_addr(selector: u32) -> AudioObjectPropertyAddress {
        AudioObjectPropertyAddress {
            mSelector: selector,
            mScope: kAudioObjectPropertyScopeGlobal,
            mElement: kAudioObjectPropertyElementMaster,
        }
    }

    unsafe fn device_string(dev: AudioObjectID, selector: u32) -> Option<String> {
        let addr = global_addr(selector);
        let mut s: CFStringRef = ptr::null();
        let mut size = std::mem::size_of::<CFStringRef>() as u32;
        if AudioObjectGetPropertyData(
            dev,
            &addr,
            0,
            ptr::null(),
            &mut size,
            &mut s as *mut _ as *mut _,
        ) != 0
            || s.is_null()
        {
            return None;
        }
        let out = cfstring_to_string(s);
        CFRelease(s as CFTypeRef);
        out
    }

    /// Channel count in one scope — the direction filter. A device with zero input channels is
    /// not in cpal's `input_devices()` list, so it must not consume an ordinal here either.
    unsafe fn channels(dev: AudioObjectID, scope: u32) -> u32 {
        let addr = AudioObjectPropertyAddress {
            mSelector: kAudioDevicePropertyStreamConfiguration,
            mScope: scope,
            mElement: kAudioObjectPropertyElementMaster,
        };
        let mut size = 0u32;
        if AudioObjectGetPropertyDataSize(dev, &addr, 0, ptr::null(), &mut size) != 0 || size == 0 {
            return 0;
        }
        let mut buf = vec![0u8; size as usize];
        if AudioObjectGetPropertyData(
            dev,
            &addr,
            0,
            ptr::null(),
            &mut size,
            buf.as_mut_ptr() as *mut _,
        ) != 0
        {
            return 0;
        }
        let list = &*(buf.as_ptr() as *const AudioBufferList);
        std::slice::from_raw_parts(list.mBuffers.as_ptr(), list.mNumberBuffers as usize)
            .iter()
            .map(|b| b.mNumberChannels)
            .sum()
    }

    /// Device locations keyed by the SAME disambiguated name the picker stores.
    ///
    /// Alignment with the picker is by name-plus-ordinal, not by raw index: this walks
    /// CoreAudio's `kAudioHardwarePropertyDevices` in order, filters to the requested direction,
    /// and applies [`crate::audiodev::disambiguate_names`] — the identical transformation
    /// `available_devices` applies to cpal's list. It lines up because cpal's macOS enumerator
    /// reads that same property in that same order, so both sides see one ordering; and because
    /// the key is the full disambiguated string, a mismatch degrades to a missing entry (no
    /// label) rather than to a wrong rig name.
    pub fn audio_locations(input: bool) -> HashMap<String, u32> {
        let mut names = Vec::new();
        let mut locs = Vec::new();
        unsafe {
            let addr = global_addr(kAudioHardwarePropertyDevices);
            let mut size = 0u32;
            if AudioObjectGetPropertyDataSize(
                kAudioObjectSystemObject,
                &addr,
                0,
                ptr::null(),
                &mut size,
            ) != 0
            {
                return HashMap::new();
            }
            let count = size as usize / std::mem::size_of::<AudioObjectID>();
            let mut ids = vec![0 as AudioObjectID; count];
            if AudioObjectGetPropertyData(
                kAudioObjectSystemObject,
                &addr,
                0,
                ptr::null(),
                &mut size,
                ids.as_mut_ptr() as *mut _,
            ) != 0
            {
                return HashMap::new();
            }
            let scope = if input {
                kAudioObjectPropertyScopeInput
            } else {
                kAudioObjectPropertyScopeOutput
            };
            for id in ids {
                if channels(id, scope) == 0 {
                    continue;
                }
                let Some(name) = device_string(id, kAudioObjectPropertyName) else {
                    continue;
                };
                let loc = device_string(id, kAudioDevicePropertyDeviceUID)
                    .as_deref()
                    .and_then(location_from_audio_uid);
                names.push(name);
                locs.push(loc);
            }
        }
        crate::audiodev::disambiguate_names(names)
            .into_iter()
            .zip(locs)
            .filter_map(|(n, l)| l.map(|l| (n, l)))
            .collect()
    }
}

#[cfg(target_os = "macos")]
pub use imp::{audio_locations, serial_interfaces, serial_locations};

/// Non-macOS: no USB topology source, so nothing is known and every label is left alone.
///
/// Windows could do this through SetupAPI device instance paths and Linux through sysfs, but
/// neither is written; the picker there keeps the `" #2"` ordinal it has always had.
#[cfg(not(target_os = "macos"))]
pub fn serial_locations() -> std::collections::HashMap<String, u32> {
    std::collections::HashMap::new()
}

/// Non-macOS counterpart of [`audio_locations`] — always empty. See [`serial_locations`].
#[cfg(not(target_os = "macos"))]
pub fn audio_locations(_input: bool) -> std::collections::HashMap<String, u32> {
    std::collections::HashMap::new()
}

/// Non-macOS counterpart of [`serial_interfaces`] — always empty. See [`serial_locations`].
#[cfg(not(target_os = "macos"))]
pub fn serial_interfaces() -> std::collections::HashMap<String, u32> {
    std::collections::HashMap::new()
}

/// The three maps a serial-port picker needs, read together.
///
/// Convenience with a point: each of the three functions walks the IO registry independently, and
/// a caller that wants all three would otherwise sweep it three times per refresh — and could see
/// three DIFFERENT moments if a cable moved in between, which is how a port ends up annotated with
/// another rig's codec. Returns `(interfaces, serial_locations, input_audio_locations)`; all three
/// are empty off macOS and on any Mac where the registry says nothing, and the caller must treat
/// empty as "unknown", not as "nothing is paired".
pub fn serial_topology() -> (
    std::collections::HashMap<String, u32>,
    std::collections::HashMap<String, u32>,
    std::collections::HashMap<String, u32>,
) {
    (
        serial_interfaces(),
        serial_locations(),
        audio_locations(true),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parent_hub_clears_only_the_last_tier() {
        // The real ON8ST paths: each rig's CAT bridge and codec are siblings.
        assert_eq!(parent_hub(0x111000), 0x110000, "FT-710 CAT");
        assert_eq!(parent_hub(0x112000), 0x110000, "FT-710 audio");
        assert_eq!(parent_hub(0x121000), 0x120000, "FTX-1 CAT");
        assert_eq!(parent_hub(0x122000), 0x120000, "FTX-1 audio");
        // The two rigs must NOT collapse together — that would label both codecs for one radio.
        assert_ne!(parent_hub(0x112000), parent_hub(0x122000));
        // Deeper nesting still only loses its last tier.
        assert_eq!(parent_hub(0x14321000), 0x14320000);
        // A root-port device has no hub; comparing it to itself is the right degenerate answer.
        assert_eq!(parent_hub(0), 0);
    }

    #[test]
    fn location_is_read_out_of_a_real_coreaudio_uid() {
        assert_eq!(
            location_from_audio_uid(
                "AppleUSBAudioEngine:C-Media Electronics Inc.:USB Audio Device:112000:2,1"
            ),
            Some(0x112000)
        );
        assert_eq!(
            location_from_audio_uid(
                "AppleUSBAudioEngine:C-Media Electronics Inc.:USB Audio Device:122000:2,1"
            ),
            Some(0x122000)
        );
        // Built-in, aggregate and virtual devices carry no port path and belong to no rig.
        assert_eq!(location_from_audio_uid("BuiltInSpeakerDevice"), None);
        assert_eq!(location_from_audio_uid("AppleAggregateDevice:0"), None);
        // A single digit is a channel-layout field, not a port path — never accept it as one.
        assert_eq!(location_from_audio_uid("Some:Device:2"), None);
    }

    /// The registry walk itself is NOT tested here, and that is a statement rather than a gap: CI
    /// has no Mac with two radios plugged into it, so any assertion about what `serial_locations`
    /// returns would pass vacuously on an empty map and prove nothing. What is testable is that
    /// nothing DECIDES anything on the walk's behalf — the two functions above are the whole of
    /// the arithmetic, and every consumer takes the maps as parameters so its own tests can
    /// inject the numbers measured on real hardware. See `ports::collapse_usb_siblings` and
    /// `checkRigForm` for those.
    #[test]
    fn the_three_maps_are_readable_and_answer_consistently() {
        // Callable on every platform, and consistent whatever it finds: a port that has an
        // interface number must also have a location, or a consumer keyed on the pair would
        // silently drop it. Vacuous on a machine with no USB serial ports — deliberately, because
        // the alternative is a test that only passes on one desk.
        let (ifaces, locs, _audio) = serial_topology();
        for name in ifaces.keys() {
            assert!(
                locs.contains_key(name),
                "{name} has an interface number but no location — the pair key would drop it"
            );
        }
    }
}
