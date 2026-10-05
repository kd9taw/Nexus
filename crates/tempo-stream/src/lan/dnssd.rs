//! Finding the shack by name, with no new crate: DNS-SD over multicast DNS, through Windows' own
//! responder. The shack advertises with `DnsServiceRegister`; the other computer browses with
//! `DnsServiceBrowse` and reads the records the answers carry (all in dnsapi.dll). As ruled
//! on 2026-10-04 ("By name, or typed"): the shack is found by name or its address is typed, and
//! the address that last worked is tried first (the other computer's to keep).
//!
//! **The minimum Windows build.** Microsoft documents these calls for "Windows 10", on pages
//! written for its 2019 spring update (version 1903, build 18362). The `windows` crate binds every
//! call it declares as a load-time import, and a Windows that lacks one refuses to start the
//! program at all, so these are looked up by name when first used instead ([`find`],
//! [`advertise`]). Where they are missing, or fail, nothing is found by name and the address is
//! typed. Measured present on build 26200.
//!
//! **What a shack advertises**, only while Remote over this network listens, and only on the
//! adapter it listens on ([`Record`]): the service `_nexus-remote._tcp`; an instance named "Nexus"
//! and the first eight characters of its LAN key's fingerprint, never the computer's own name,
//! which often carries a person's; a host name made the same way; its port; and a TXT record with
//! the LAN protocol it speaks (`v`), the first 64 bits of the fingerprint (`k`), so a computer can
//! tell its own shack among several before it connects, and the address it listens at (`a`).
//!
//! **Why the address is in the TXT record.** Windows' responder publishes an address only for the
//! computer's own name, which also answers with every adapter's address, WSL's virtual switch's
//! among them. For the host name advertised here it publishes none (measured on Windows 11: the
//! host does not resolve, and `DnsServiceResolve` waits for its address until it is cancelled),
//! while the browse's own answer carries the PTR, SRV and TXT records. So the TXT record says the
//! one address the shack listens at, and the browse's records are read as they come.
//!
//! **A found shack is a hint, never an identity.** The computer connects only to the key it pinned
//! at pairing, so a forged answer costs one failed handshake. Only a private IPv4 address (the
//! shack listens nowhere else) and a port that is not a system one are taken ([`found`]).
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use serde::Serialize;

use super::listenable;

/// The service a shack advertises while Remote over this network listens.
pub const SERVICE: &str = "_nexus-remote._tcp.local";

/// What a shack advertises.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    /// The instance's own name: "Nexus 3F2A 9B1C".
    pub name: String,
    /// The host the address is published under: "nexus-3f2a9b1c.local".
    pub host: String,
    pub address: SocketAddrV4,
    /// The TXT record's keys and values: `v`, the LAN protocol; `k`, the key's first 64 bits;
    /// `a`, the address it listens at.
    pub txt: Vec<(String, String)>,
    /// The interface it is advertised on: the one the shack listens on.
    pub interface: u32,
}

impl Record {
    /// The record of a shack whose LAN key has `fingerprint` (SHA-256 of its SPKI, hex), speaking
    /// LAN `protocol`, listening at `address` on `interface`. `None` for a fingerprint shorter than
    /// 64 bits or not hexadecimal.
    pub fn new(
        fingerprint: &str,
        protocol: u8,
        address: SocketAddrV4,
        interface: u32,
    ) -> Option<Self> {
        let key = fingerprint
            .get(..16)
            .filter(|key| key.bytes().all(|b| b.is_ascii_hexdigit()))?
            .to_ascii_lowercase();
        let tag = key[..8].to_ascii_uppercase();
        Some(Self {
            name: format!("Nexus {} {}", &tag[..4], &tag[4..]),
            host: format!("nexus-{}.local", &key[..8]),
            address,
            txt: vec![
                ("v".into(), protocol.to_string()),
                ("k".into(), key),
                ("a".into(), address.ip().to_string()),
            ],
            interface,
        })
    }

    /// The whole instance name Windows registers: the name, then the service.
    pub fn instance(&self) -> String {
        format!("{}.{SERVICE}", self.name)
    }
}

/// A shack found on this computer's networks.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    /// The name it advertises: "Nexus 3F2A 9B1C".
    pub name: String,
    pub address: SocketAddrV4,
    /// The LAN protocol it speaks.
    pub protocol: u32,
    /// The first 64 bits of its key's fingerprint, sixteen lowercase hex characters: the start of
    /// the fingerprint a computer pinned at pairing, if it is that shack.
    pub key: String,
}

/// One record a browse brings back, as the shacks are read from them ([`shacks`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Seen {
    /// A PTR record: an instance of the service.
    Instance(String),
    /// An instance's SRV record: its port.
    Port(String, u16),
    /// An instance's TXT record: its strings, each `key=value`.
    Text(String, Vec<String>),
}

/// The shacks a browse's records describe: each instance with its port and its TXT record, the
/// latest of each where an answer came more than once, read by [`found`]. An instance with no SRV
/// or no TXT record among them is not one: its address can still be typed.
pub fn shacks(seen: &[Seen]) -> Vec<Found> {
    let same = |a: &str, b: &str| {
        a.trim_end_matches('.')
            .eq_ignore_ascii_case(b.trim_end_matches('.'))
    };
    let mut found: Vec<Found> = seen
        .iter()
        .filter_map(|record| match record {
            Seen::Instance(instance) => Some(instance),
            _ => None,
        })
        .filter_map(|instance| {
            let port = seen.iter().rev().find_map(|record| match record {
                Seen::Port(owner, port) if same(owner, instance) => Some(*port),
                _ => None,
            })?;
            let txt: Vec<(String, String)> = seen
                .iter()
                .rev()
                .find_map(|record| match record {
                    Seen::Text(owner, strings) if same(owner, instance) => Some(strings),
                    _ => None,
                })?
                .iter()
                .filter_map(|string| string.split_once('='))
                .map(|(key, value)| (key.to_string(), value.to_string()))
                .collect();
            found(instance, port, &txt)
        })
        .collect();
    found.sort();
    found.dedup();
    found
}

/// An instance, its port and its TXT record read as a shack's: `None` unless it is one. Not this
/// service, no name, a system port, no protocol, no 64-bit key, and no address or one the shack
/// never listens at (anything but private IPv4) all read as none.
pub fn found(instance: &str, port: u16, txt: &[(String, String)]) -> Option<Found> {
    let instance = instance.trim_end_matches('.');
    let suffix = format!(".{SERVICE}");
    let at = instance.len().checked_sub(suffix.len())?;
    let (name, tail) = (instance.get(..at)?, instance.get(at..)?);
    if name.is_empty() || !tail.eq_ignore_ascii_case(&suffix) {
        return None;
    }
    let value = |key: &str| {
        txt.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    };
    let protocol = value("v")?.parse().ok()?;
    let key = value("k")
        .filter(|k| k.len() == 16 && k.bytes().all(|b| b.is_ascii_hexdigit()))?
        .to_ascii_lowercase();
    let ip = value("a")?
        .parse::<Ipv4Addr>()
        .ok()
        .filter(|ip| listenable(*ip))?;
    (port >= 1024).then(|| Found {
        name: name.to_string(),
        address: SocketAddrV4::new(ip, port),
        protocol,
        key,
    })
}

/// Windows' own name service cannot be used here: not Windows, a build without the calls, or a
/// call that failed. The shack's address is typed instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Unavailable;

/// A shack's advert, withdrawn when this is dropped. Dropping it waits, two seconds at most, for
/// Windows to say the advert is gone, so drop it on a thread that may wait.
pub struct Advert {
    #[cfg(windows)]
    registration: system::Registration,
}

impl Advert {
    /// Has Windows said the advert stands (`Some(true)`), or refused it (`Some(false)`)? `None`
    /// until it says.
    pub fn standing(&self) -> Option<bool> {
        #[cfg(windows)]
        {
            self.registration.standing()
        }
        #[cfg(not(windows))]
        {
            None
        }
    }
}

/// Advertise `record` until the advert is dropped. `None` where Windows' name service cannot be
/// used: the other computer then types the address.
pub fn advertise(record: &Record) -> Option<Advert> {
    #[cfg(windows)]
    {
        system::register(record).map(|registration| Advert { registration })
    }
    #[cfg(not(windows))]
    {
        let _ = record;
        None
    }
}

/// The shacks that answer on this computer's networks within `wait`, their records with them.
/// Blocks for that long: call it off any thread that must not wait.
pub fn find(wait: Duration) -> Result<Vec<Found>, Unavailable> {
    #[cfg(windows)]
    {
        system::find(wait)
    }
    #[cfg(not(windows))]
    {
        let _ = wait;
        Err(Unavailable)
    }
}

#[cfg(windows)]
mod system {
    use std::ffi::c_void;
    use std::mem::ManuallyDrop;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::sync::{Mutex, OnceLock};
    use std::time::{Duration, Instant};

    use windows::core::{s, w, PCSTR, PCWSTR, PWSTR};
    use windows::Win32::Foundation::{DNS_REQUEST_PENDING, ERROR_SUCCESS, HANDLE, HMODULE};
    use windows::Win32::NetworkManagement::Dns::{
        DnsFree, DnsFreeRecordList, DNS_QUERY_REQUEST_VERSION1, DNS_RECORDW,
        DNS_SERVICE_BROWSE_REQUEST, DNS_SERVICE_BROWSE_REQUEST_0, DNS_SERVICE_CANCEL,
        DNS_SERVICE_INSTANCE, DNS_SERVICE_REGISTER_REQUEST, DNS_TYPE_PTR, DNS_TYPE_SRV,
        DNS_TYPE_TEXT, IP6_ADDRESS,
    };
    use windows::Win32::System::LibraryLoader::{
        GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32,
    };

    use super::{Found, Record, Seen, Unavailable, SERVICE};

    /// The most records kept from one browse: a flood of answers can hide a shack, never grow
    /// without end, and the address can still be typed.
    const MOST: usize = 256;
    /// How long a withdrawn advert waits for Windows to say it is gone.
    const WITHDRAW: Duration = Duration::from_secs(2);

    type Browse = unsafe extern "system" fn(
        *const DNS_SERVICE_BROWSE_REQUEST,
        *mut DNS_SERVICE_CANCEL,
    ) -> i32;
    type Cancel = unsafe extern "system" fn(*const DNS_SERVICE_CANCEL) -> i32;
    type Register = unsafe extern "system" fn(
        *const DNS_SERVICE_REGISTER_REQUEST,
        *mut DNS_SERVICE_CANCEL,
    ) -> u32;
    type Construct = unsafe extern "system" fn(
        PCWSTR,
        PCWSTR,
        *const u32,
        *const IP6_ADDRESS,
        u16,
        u16,
        u16,
        u32,
        *const PCWSTR,
        *const PCWSTR,
    ) -> *mut DNS_SERVICE_INSTANCE;
    type Free = unsafe extern "system" fn(*const DNS_SERVICE_INSTANCE);

    /// The calls, each with the signature windns.h declares for it.
    struct Calls {
        browse: Browse,
        browse_cancel: Cancel,
        register: Register,
        deregister: Register,
        construct: Construct,
        free: Free,
    }

    /// The calls, looked up in System32's dnsapi.dll the first time they are wanted, which then
    /// stays loaded. `None` when any one is missing: a Windows build from before them.
    fn calls() -> Option<&'static Calls> {
        static CALLS: OnceLock<Option<Calls>> = OnceLock::new();
        CALLS
            .get_or_init(|| {
                // SAFETY: a system DLL by name, from System32 only.
                let dll =
                    unsafe { LoadLibraryExW(w!("dnsapi.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32) }
                        .ok()?;
                // SAFETY: each export is taken as the signature windns.h declares for it.
                unsafe {
                    Some(Calls {
                        browse: export(dll, s!("DnsServiceBrowse"))?,
                        browse_cancel: export(dll, s!("DnsServiceBrowseCancel"))?,
                        register: export(dll, s!("DnsServiceRegister"))?,
                        deregister: export(dll, s!("DnsServiceDeRegister"))?,
                        construct: export(dll, s!("DnsServiceConstructInstance"))?,
                        free: export(dll, s!("DnsServiceFreeInstance"))?,
                    })
                }
            })
            .as_ref()
    }

    /// The export `name` of `dll`, as a function of type `T`.
    ///
    /// # Safety
    /// `T` is an `extern "system" fn` type with the export's own signature.
    unsafe fn export<T: Copy>(dll: HMODULE, name: PCSTR) -> Option<T> {
        // SAFETY: a name, looked up in a loaded module.
        let found = unsafe { GetProcAddress(dll, name) }?;
        assert_eq!(std::mem::size_of::<T>(), std::mem::size_of_val(&found));
        // SAFETY: both are function pointers of the same size, and the caller names the type.
        Some(unsafe { std::mem::transmute_copy(&found) })
    }

    /// What Windows' callbacks say, sent to whoever asked. Each request carries a number as its
    /// context, never a pointer, so a callback that comes after its asker has gone finds nobody
    /// and touches nothing that was freed.
    enum Said {
        /// A browse's answer: the records it carried.
        Records(Vec<Seen>),
        /// A registration's answer, or its withdrawal's: the status.
        Answer(u32),
    }

    static ASKERS: Mutex<Vec<(usize, Sender<Said>)>> = Mutex::new(Vec::new());
    static NEXT: AtomicUsize = AtomicUsize::new(1);

    fn ask() -> (usize, Receiver<Said>) {
        let (tell, said) = mpsc::channel();
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        if let Ok(mut askers) = ASKERS.lock() {
            askers.push((id, tell));
        }
        (id, said)
    }

    fn tell(id: usize, said: Said) {
        if let Ok(askers) = ASKERS.lock() {
            if let Some((_, tell)) = askers.iter().find(|(asker, _)| *asker == id) {
                let _ = tell.send(said);
            }
        }
    }

    fn forget(id: usize) {
        if let Ok(mut askers) = ASKERS.lock() {
            askers.retain(|(asker, _)| *asker != id);
        }
    }

    /// A string Windows wrote, or empty for none.
    ///
    /// # Safety
    /// `text` is null or points at a null-terminated UTF-16 string.
    unsafe fn text(text: PWSTR) -> String {
        if text.is_null() {
            return String::new();
        }
        // SAFETY: as the caller promises.
        unsafe { text.to_string() }.unwrap_or_default()
    }

    unsafe extern "system" fn browsed(
        _status: u32,
        context: *const c_void,
        records: *const DNS_RECORDW,
    ) {
        let mut seen = Vec::new();
        let mut record = records;
        // SAFETY: Windows hands over a list of records, each `pNext` null or the next, and each
        // record's data is of the kind its type says.
        while let Some(found) = unsafe { record.as_ref() } {
            unsafe {
                let owner = text(found.pName);
                if found.wType == DNS_TYPE_PTR.0 {
                    seen.push(Seen::Instance(text(found.Data.PTR.pNameHost)));
                } else if found.wType == DNS_TYPE_SRV.0 {
                    seen.push(Seen::Port(owner, found.Data.SRV.wPort));
                } else if found.wType == DNS_TYPE_TEXT.0 {
                    let txt = &found.Data.TXT;
                    let strings = (0..txt.dwStringCount as usize)
                        .map(|n| text(*txt.pStringArray.as_ptr().add(n)))
                        .collect();
                    seen.push(Seen::Text(owner, strings));
                }
            }
            record = found.pNext;
        }
        if !records.is_null() {
            // SAFETY: the list is the callback's to free, as its documentation says.
            unsafe { DnsFree(Some(records.cast()), DnsFreeRecordList) };
        }
        tell(context as usize, Said::Records(seen));
    }

    unsafe extern "system" fn answered(
        status: u32,
        context: *const c_void,
        instance: *const DNS_SERVICE_INSTANCE,
    ) {
        if let (false, Some(calls)) = (instance.is_null(), calls()) {
            // SAFETY: the instance is the callback's to free, as its documentation says.
            unsafe { (calls.free)(instance) };
        }
        tell(context as usize, Said::Answer(status));
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(Some(0)).collect()
    }

    pub fn find(wait: Duration) -> Result<Vec<Found>, Unavailable> {
        let calls = calls().ok_or(Unavailable)?;
        let (id, said) = ask();
        let query = wide(SERVICE);
        let request = DNS_SERVICE_BROWSE_REQUEST {
            Version: DNS_QUERY_REQUEST_VERSION1.0,
            InterfaceIndex: 0,
            QueryName: PCWSTR(query.as_ptr()),
            Anonymous: DNS_SERVICE_BROWSE_REQUEST_0 {
                pBrowseCallback: Some(browsed),
            },
            pQueryContext: id as *mut c_void,
        };
        let mut cancel = DNS_SERVICE_CANCEL::default();
        // SAFETY: the request, its name and the cancel handle outlive the browse, which is
        // cancelled below before any of them goes.
        let started = unsafe { (calls.browse)(&request, &mut cancel) };
        if started != DNS_REQUEST_PENDING {
            forget(id);
            return Err(Unavailable);
        }
        let deadline = Instant::now() + wait;
        let mut seen = Vec::new();
        while let Ok(heard) = said.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            if let Said::Records(more) = heard {
                seen.extend(more);
                seen.truncate(MOST);
            }
        }
        // SAFETY: the handle the browse filled in. Windows answers the cancel once more, with
        // nothing, and that answer finds nobody once the asker is forgotten.
        unsafe { (calls.browse_cancel)(&cancel) };
        forget(id);
        Ok(super::shacks(&seen))
    }

    /// An advert Windows holds until it is withdrawn ([`Drop`]).
    pub struct Registration {
        id: usize,
        said: Receiver<Said>,
        /// The request the advert was made with, and its instance. Windows reads both until it says
        /// the advert is withdrawn; if it never says, they are left where they are for good.
        request: ManuallyDrop<Box<DNS_SERVICE_REGISTER_REQUEST>>,
        /// What Windows said of the registration, and how many answers it has given.
        heard: Mutex<(Option<bool>, usize)>,
    }

    // SAFETY: the request's pointers are Windows' instance, read only by Windows and by `Drop`,
    // and a context that is a number; nothing in it is tied to the thread that made it.
    unsafe impl Send for Registration {}

    pub fn register(record: &Record) -> Option<Registration> {
        let calls = calls()?;
        let name = wide(&record.instance());
        let host = wide(&record.host);
        let keys: Vec<Vec<u16>> = record.txt.iter().map(|(key, _)| wide(key)).collect();
        let values: Vec<Vec<u16>> = record.txt.iter().map(|(_, value)| wide(value)).collect();
        let key_list: Vec<PCWSTR> = keys.iter().map(|key| PCWSTR(key.as_ptr())).collect();
        let value_list: Vec<PCWSTR> = values.iter().map(|value| PCWSTR(value.as_ptr())).collect();
        let ip4 = u32::from(*record.address.ip()).to_be();
        // SAFETY: every string is null-terminated and outlives the call, which copies them into
        // the instance it makes; the lists hold `txt.len()` strings each.
        let instance = unsafe {
            (calls.construct)(
                PCWSTR(name.as_ptr()),
                PCWSTR(host.as_ptr()),
                &ip4,
                std::ptr::null(),
                record.address.port(),
                0,
                0,
                key_list.len() as u32,
                key_list.as_ptr(),
                value_list.as_ptr(),
            )
        };
        if instance.is_null() {
            return None;
        }
        // SAFETY: the instance the call just made, which nothing else holds yet.
        unsafe { (*instance).dwInterfaceIndex = record.interface };
        let (id, said) = ask();
        let request = Box::new(DNS_SERVICE_REGISTER_REQUEST {
            Version: DNS_QUERY_REQUEST_VERSION1.0,
            InterfaceIndex: record.interface,
            pServiceInstance: instance,
            pRegisterCompletionCallback: Some(answered),
            pQueryContext: id as *mut c_void,
            hCredentials: HANDLE::default(),
            unicastEnabled: false.into(),
        });
        // SAFETY: the request and its instance stay where they are until the advert is withdrawn.
        let started = unsafe { (calls.register)(&*request, std::ptr::null_mut()) };
        if started != DNS_REQUEST_PENDING as u32 {
            forget(id);
            // SAFETY: Windows took nothing, so the instance is still this function's.
            unsafe { (calls.free)(instance) };
            return None;
        }
        Some(Registration {
            id,
            said,
            request: ManuallyDrop::new(request),
            heard: Mutex::new((None, 0)),
        })
    }

    /// Take in one thing Windows said of an advert: its first answer is the registration's.
    fn hear(heard: &mut (Option<bool>, usize), said: Said) {
        if let Said::Answer(status) = said {
            if heard.1 == 0 {
                heard.0 = Some(status == ERROR_SUCCESS.0);
            }
            heard.1 += 1;
        }
    }

    impl Registration {
        pub fn standing(&self) -> Option<bool> {
            let mut heard = self.heard.lock().ok()?;
            while let Ok(said) = self.said.try_recv() {
                hear(&mut heard, said);
            }
            heard.0
        }
    }

    impl Drop for Registration {
        fn drop(&mut self) {
            let Some(calls) = calls() else {
                return;
            };
            let mut heard = self.heard.lock().map(|h| *h).unwrap_or((None, 0));
            while let Ok(said) = self.said.try_recv() {
                hear(&mut heard, said);
            }
            // SAFETY: the request the advert was made with, which has not moved.
            let started = unsafe { (calls.deregister)(&**self.request, std::ptr::null_mut()) };
            // Done once Windows has answered the withdrawal too (its second answer), or when it
            // took no withdrawal to answer.
            let mut done = started != DNS_REQUEST_PENDING as u32;
            let deadline = Instant::now() + WITHDRAW;
            while !done {
                match self
                    .said
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                {
                    Ok(said) => {
                        hear(&mut heard, said);
                        done = heard.1 >= 2;
                    }
                    Err(_) => break,
                }
            }
            forget(self.id);
            if done {
                // SAFETY: Windows has finished with the request and its instance, which are this
                // advert's again; neither is touched after this.
                unsafe {
                    (calls.free)(self.request.pServiceInstance);
                    ManuallyDrop::drop(&mut self.request);
                }
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::super::super::{look, PORT};
        use super::*;

        /// ★ On a real Windows network stack, by hand (it advertises on this computer's network
        /// for a few seconds): the calls are there on this build, a record advertised on the
        /// adapter the listener would use is found by browsing and read back whole, and once it
        /// is withdrawn it is found no more. Each answer is printed (`--nocapture`).
        #[test]
        #[ignore = "advertises on this computer's own network: run by hand on Windows"]
        fn an_advert_is_found_by_name_and_withdrawn() {
            assert!(
                calls().is_some(),
                "the DNS-SD calls are missing on this build"
            );
            let look = look(None);
            let choice = look.chosen().expect("no network the listener would use");
            let address = std::net::SocketAddrV4::new(choice.network.address(), PORT);
            let record = Record::new("00c0ffee00c0ffee", 1, address, choice.index).unwrap();
            let advert = register(&record).expect("not registered");
            let started = Instant::now();
            while advert.standing().is_none() && started.elapsed() < Duration::from_secs(5) {
                std::thread::sleep(Duration::from_millis(50));
            }
            println!(
                "registered in {:?}: {:?}",
                started.elapsed(),
                advert.standing()
            );
            assert_eq!(advert.standing(), Some(true));
            let found = find(Duration::from_secs(3)).unwrap();
            println!("found: {found:?}");
            let ours = found
                .iter()
                .find(|f| f.key == "00c0ffee00c0ffee")
                .expect("our own advert was not found");
            assert_eq!(ours.name, "Nexus 00C0 FFEE");
            assert_eq!(ours.address, address);
            assert_eq!(ours.protocol, 1);
            let withdrawing = Instant::now();
            drop(advert);
            println!("withdrawn in {:?}", withdrawing.elapsed());
            // Windows' own browse still answers from its cache for a moment after the goodbye.
            std::thread::sleep(Duration::from_secs(3));
            let after = find(Duration::from_secs(3)).unwrap();
            println!("after: {after:?}");
            assert!(
                after.iter().all(|f| f.key != "00c0ffee00c0ffee"),
                "still found once withdrawn"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FINGERPRINT: &str = "3f2a9b1c55d4e6f70123456789abcdef0123456789abcdef0123456789abcdef";

    fn at(text: &str) -> SocketAddrV4 {
        text.parse().unwrap()
    }

    fn pairs(txt: &[(&str, &str)]) -> Vec<(String, String)> {
        txt.iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect()
    }

    /// ★ The record a shack advertises: "Nexus" and the first eight characters of its key's
    /// fingerprint, never the computer's own name; a host made the same way; the protocol, the
    /// key's first 64 bits and the address it listens at in its TXT record. A fingerprint too
    /// short, or not hex, makes none.
    #[test]
    fn a_shack_advertises_its_key_and_never_the_computers_name() {
        let record = Record::new(FINGERPRINT, 1, at("192.168.1.20:42075"), 9).unwrap();
        assert_eq!(record.name, "Nexus 3F2A 9B1C");
        assert_eq!(
            record.instance(),
            "Nexus 3F2A 9B1C._nexus-remote._tcp.local"
        );
        assert_eq!(record.host, "nexus-3f2a9b1c.local");
        assert_eq!(record.address, at("192.168.1.20:42075"));
        assert_eq!(record.interface, 9);
        assert_eq!(
            record.txt,
            pairs(&[("v", "1"), ("k", "3f2a9b1c55d4e6f7"), ("a", "192.168.1.20")])
        );
        // Upper-case hex reads the same.
        assert_eq!(
            Record::new(&FINGERPRINT.to_uppercase(), 1, at("192.168.1.20:42075"), 9),
            Some(record)
        );
        for bad in [
            "",
            "3f2a9b1c55d4e6f",
            "3f2a9b1c55d4e6fg0000",
            "Nexus 3F2A 9B1C",
        ] {
            assert_eq!(
                Record::new(bad, 1, at("192.168.1.20:42075"), 9),
                None,
                "{bad}"
            );
        }
    }

    /// ★ What a computer reads back is what the shack advertised, field for field, with Windows'
    /// trailing dot on the name or without it.
    #[test]
    fn an_advertised_record_reads_back_as_the_shack() {
        let record = Record::new(FINGERPRINT, 1, at("192.168.1.20:42075"), 9).unwrap();
        let want = Found {
            name: "Nexus 3F2A 9B1C".into(),
            address: at("192.168.1.20:42075"),
            protocol: 1,
            key: "3f2a9b1c55d4e6f7".into(),
        };
        for instance in [record.instance(), format!("{}.", record.instance())] {
            assert_eq!(
                found(&instance, 42075, &record.txt),
                Some(want.clone()),
                "{instance}"
            );
        }
    }

    /// ★ A forged or foreign answer is no shack: another service, no name, an address the shack
    /// never listens at (public, loopback, carrier NAT, none, not an address), a system port, no
    /// protocol or a bad one, no key or a short one. CONTROL: the same answer with each fault
    /// mended is one.
    #[test]
    fn only_a_shacks_own_kind_of_record_is_found() {
        let txt = |v: &str, k: &str, a: &str| pairs(&[("v", v), ("k", k), ("a", a)]);
        let good = txt("1", "3f2a9b1c55d4e6f7", "192.168.1.20");
        let name = "Nexus 3F2A 9B1C._nexus-remote._tcp.local";
        assert!(found(name, 42075, &good).is_some(), "control");
        for (instance, port, txt) in [
            ("Printer._ipp._tcp.local", 42075, good.clone()),
            ("._nexus-remote._tcp.local", 42075, good.clone()),
            ("_nexus-remote._tcp.local", 42075, good.clone()),
            (name, 42075, txt("1", "3f2a9b1c55d4e6f7", "203.0.113.5")),
            (name, 42075, txt("1", "3f2a9b1c55d4e6f7", "127.0.0.1")),
            (name, 42075, txt("1", "3f2a9b1c55d4e6f7", "100.85.1.2")),
            (name, 42075, txt("1", "3f2a9b1c55d4e6f7", "nexus.local")),
            (name, 42075, pairs(&[("v", "1"), ("k", "3f2a9b1c55d4e6f7")])),
            (name, 80, good.clone()),
            (name, 42075, txt("one", "3f2a9b1c55d4e6f7", "192.168.1.20")),
            (
                name,
                42075,
                pairs(&[("k", "3f2a9b1c55d4e6f7"), ("a", "192.168.1.20")]),
            ),
            (name, 42075, txt("1", "3f2a9b1c", "192.168.1.20")),
            (name, 42075, txt("1", "3f2a9b1c55d4e6fz", "192.168.1.20")),
            (name, 42075, pairs(&[("v", "1"), ("a", "192.168.1.20")])),
        ] {
            assert_eq!(
                found(instance, port, &txt),
                None,
                "{instance} {port} {txt:?}"
            );
        }
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// ★ The shacks a browse's records describe, as Windows delivered them on Windows 11 (a first
    /// answer carrying the PTR alone, a second the PTR with the SRV and TXT records): each shack
    /// once, with the latest port and TXT where an answer came again; nothing for a foreign
    /// service, for an instance whose SRV or TXT never came, or for a TXT with no address.
    #[test]
    fn a_browses_records_read_as_each_shack_once() {
        let den = "Nexus 3F2A 9B1C._nexus-remote._tcp.local";
        let shack = "Nexus 00C0 FFEE._nexus-remote._tcp.local";
        let den_txt = strings(&["v=1", "k=3f2a9b1c55d4e6f7", "a=192.168.1.20"]);
        let seen = vec![
            Seen::Instance(den.into()),
            Seen::Instance(den.into()),
            Seen::Port(den.into(), 42075),
            Seen::Text(den.into(), den_txt.clone()),
            // The other shack restarted on another port: its later answer counts.
            Seen::Instance(shack.into()),
            Seen::Port(shack.into(), 42075),
            Seen::Text(
                shack.into(),
                strings(&["v=1", "k=00c0ffee00c0ffee", "a=10.0.0.5"]),
            ),
            Seen::Port(format!("{shack}."), 42080),
            // A printer, an instance with no TXT, and one whose TXT has no address.
            Seen::Instance("Printer._nexus-remote._tcp.local.x".into()),
            Seen::Instance("Nexus 1111 2222._nexus-remote._tcp.local".into()),
            Seen::Port("Nexus 1111 2222._nexus-remote._tcp.local".into(), 42075),
            Seen::Instance("Nexus 3333 4444._nexus-remote._tcp.local".into()),
            Seen::Port("Nexus 3333 4444._nexus-remote._tcp.local".into(), 42075),
            Seen::Text(
                "Nexus 3333 4444._nexus-remote._tcp.local".into(),
                strings(&["v=1", "k=3333444455556666"]),
            ),
        ];
        assert_eq!(
            shacks(&seen),
            vec![
                Found {
                    name: "Nexus 00C0 FFEE".into(),
                    address: at("10.0.0.5:42080"),
                    protocol: 1,
                    key: "00c0ffee00c0ffee".into(),
                },
                Found {
                    name: "Nexus 3F2A 9B1C".into(),
                    address: at("192.168.1.20:42075"),
                    protocol: 1,
                    key: "3f2a9b1c55d4e6f7".into(),
                },
            ]
        );
        // CONTROL: the SRV and TXT alone, without the PTR naming the instance, make no shack.
        assert!(shacks(&[
            Seen::Port(den.into(), 42075),
            Seen::Text(den.into(), den_txt)
        ])
        .is_empty());
    }

    /// Off Windows there is no name service: finding fails to the typed address, and nothing is
    /// advertised.
    #[cfg(not(windows))]
    #[test]
    fn off_windows_the_address_is_typed() {
        assert_eq!(find(Duration::from_millis(1)), Err(Unavailable));
        let record = Record::new(FINGERPRINT, 1, at("192.168.1.20:42075"), 9).unwrap();
        assert!(advertise(&record).is_none());
    }
}
