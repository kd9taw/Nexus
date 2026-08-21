//! The real OmniRig COM client — **the only Windows-specific code in this feature**.
//!
//! Everything else in [`super`] is platform-neutral and unit-tested on Linux against a mock
//! [`OmniRigClient`]; this file is the thin object behind that trait. It is compiled only on
//! Windows (`#[cfg(windows)]` at the module declaration), so off Windows the feature is
//! *absent* rather than stubbed with something that pretends to work.
//!
//! **Apartment.** `CoInitializeEx(COINIT_APARTMENTTHREADED)` runs here, on the thread that
//! creates the object, and [`Apartment`] is the LAST field of [`OmniRigCom`] so Rust's
//! declaration-order drop releases both interfaces *before* `CoUninitialize`. Nothing in this
//! file may be moved to another thread — the whole reason [`super::OmniWorker`] exists is to
//! give the object one thread for its entire life.
//!
//! **Late binding, by name.** OmniRig's `IOmniRigX`/`IRigX` are dual interfaces, but Nexus
//! drives them through `IDispatch` rather than declaring their vtables: the DISPIDs are
//! resolved once at connect with `GetIDsOfNames` and cached, so the per-call cost is one
//! `Invoke` and a mis-typed property name fails loudly at connect instead of corrupting a
//! stack at call time. Names and semantics are read off OmniRig's own type library
//! (`OmniRig_TLB.pas`, VE3NEA/OmniRig) — see [`super`]'s header.

use windows::core::{Interface, BSTR, GUID, PCWSTR, VARIANT};
use windows::Win32::Foundation::{CO_E_CLASSSTRING, REGDB_E_CLASSNOTREG};
use windows::Win32::System::Com::{
    CLSIDFromProgID, CoCreateInstance, CoInitializeEx, CoUninitialize, IDispatch, CLSCTX_ALL,
    COINIT_APARTMENTTHREADED, DISPATCH_PROPERTYGET, DISPATCH_PROPERTYPUT, DISPPARAMS,
};
use windows::Win32::System::Ole::DISPID_PROPERTYPUT;
use windows::Win32::System::Variant::{VARENUM, VT_DISPATCH};

use super::{param, OmniError, OmniMode, OmniRigClient, OmniStatus, RigSlot, PROGID};

/// `LOCALE_USER_DEFAULT`. `IDispatch` wants an LCID for name lookup and for `Invoke`;
/// OmniRig's names are ASCII and locale-invariant, so this is a formality — but a zero LCID
/// is *not* the same thing and some dispatch implementations reject it.
const LOCALE_USER_DEFAULT: u32 = 0x0400;

/// Owns this thread's COM initialisation. Dropped last (see the module header).
struct Apartment;

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: paired 1:1 with the successful CoInitializeEx in `connect`, on this thread.
        unsafe { CoUninitialize() };
    }
}

/// The DISPIDs of every property we use on the rig object, resolved once.
struct RigIds {
    status: i32,
    status_str: i32,
    freq: i32,
    mode: i32,
    tx: i32,
    split: i32,
}

/// A live OmniRig connection for one rig slot.
pub struct OmniRigCom {
    /// The `OmniRigX` server object. Held for the connection's life: releasing it would let
    /// the server shut down while we still hold a rig object out of it.
    _server: IDispatch,
    rig: IDispatch,
    ids: RigIds,
    slot: RigSlot,
    /// LAST field on purpose — `CoUninitialize` must run after both interfaces are released.
    _apartment: Apartment,
}

/// A NUL-terminated UTF-16 copy of `s`, for the `PCWSTR` arguments.
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Turn a windows-rs error into an [`OmniError`], keeping the HRESULT — support asks for it.
fn com_err(what: &str, e: &windows::core::Error) -> OmniError {
    OmniError::Com(format!("{what}: {} (0x{:08X})", e.message(), e.code().0))
}

/// `HRESULT_FROM_WIN32(ERROR_ELEVATION_REQUIRED)` — 0x800702E4. Written out as the literal
/// rather than assembled from the Win32 code because that literal is what an operator reads
/// off the screen and pastes into a bug report, and a grep for it should land here.
const E_ELEVATION_REQUIRED: u32 = 0x8007_02E4;

/// Pure, so the mapping is testable off Windows: does this HRESULT mean "that server needs
/// to run elevated and you do not"?
fn is_elevation_required(code: windows::core::HRESULT) -> bool {
    code.0 as u32 == E_ELEVATION_REQUIRED
}

/// Resolve one property/method name on `disp` to its DISPID.
fn dispid(disp: &IDispatch, name: &str) -> Result<i32, OmniError> {
    let w = wide(name);
    let names = [PCWSTR(w.as_ptr())];
    let mut id: i32 = 0;
    // SAFETY: `names` outlives the call; `id` is a valid out-slot for exactly `cnames == 1`.
    unsafe {
        disp.GetIDsOfNames(
            &GUID::zeroed(),
            names.as_ptr(),
            1,
            LOCALE_USER_DEFAULT,
            &mut id,
        )
    }
    .map_err(|e| com_err(&format!("OmniRig has no property {name:?}"), &e))?;
    Ok(id)
}

/// `IDispatch` property GET → a raw VARIANT.
fn get_prop(disp: &IDispatch, id: i32, what: &str) -> Result<VARIANT, OmniError> {
    let params = DISPPARAMS::default();
    let mut out = VARIANT::new();
    // SAFETY: no arguments; `out` is a live, empty VARIANT we own and later drop.
    unsafe {
        disp.Invoke(
            id,
            &GUID::zeroed(),
            LOCALE_USER_DEFAULT,
            DISPATCH_PROPERTYGET,
            &params,
            Some(&mut out),
            None,
            None,
        )
    }
    .map_err(|e| com_err(&format!("reading {what}"), &e))?;
    Ok(out)
}

/// `IDispatch` property PUT of one `i32` argument.
fn put_prop_i32(disp: &IDispatch, id: i32, value: i32, what: &str) -> Result<(), OmniError> {
    let mut arg = VARIANT::from(value);
    let mut named = DISPID_PROPERTYPUT;
    let params = DISPPARAMS {
        rgvarg: &mut arg,
        rgdispidNamedArgs: &mut named,
        cArgs: 1,
        cNamedArgs: 1,
    };
    // SAFETY: `arg`/`named` outlive the call and stay owned by us (a property put copies the
    // value; `arg` is a by-value i32 VARIANT, so there is nothing to transfer).
    unsafe {
        disp.Invoke(
            id,
            &GUID::zeroed(),
            LOCALE_USER_DEFAULT,
            DISPATCH_PROPERTYPUT,
            &params,
            None,
            None,
            None,
        )
    }
    .map_err(|e| com_err(&format!("setting {what}"), &e))?;
    Ok(())
}

/// Read an `i32` out of a VARIANT, naming what was being read if it is the wrong type.
fn as_i32(v: &VARIANT, what: &str) -> Result<i32, OmniError> {
    i32::try_from(v).map_err(|e| {
        OmniError::Com(format!(
            "{what} came back as something other than a number ({})",
            e.message()
        ))
    })
}

/// Pull an `IDispatch` out of a `VT_DISPATCH` VARIANT.
///
/// windows-rs 0.54's `VARIANT` converts to `IUnknown` for `VT_UNKNOWN` only, and OmniRig
/// hands back `VT_DISPATCH` — so the tag is checked and the pointer taken from the raw union
/// (`VARIANT::as_raw` is the crate's own public accessor for exactly this). The returned
/// interface is `clone`d, i.e. AddRef'd, so it stays valid after the VARIANT is dropped and
/// releases its own reference.
fn as_dispatch(v: &VARIANT, what: &str) -> Result<IDispatch, OmniError> {
    // SAFETY: reading the discriminant + matching pointer arm of a VARIANT we own.
    let (vt, ptr) = unsafe {
        let raw = v.as_raw();
        (
            raw.Anonymous.Anonymous.vt,
            raw.Anonymous.Anonymous.Anonymous.pdispVal,
        )
    };
    if VARENUM(vt) != VT_DISPATCH || ptr.is_null() {
        return Err(OmniError::Com(format!(
            "{what} did not come back as an object (VARIANT type {vt})"
        )));
    }
    // SAFETY: the pointer arm matches the VT_DISPATCH tag, so it is a live IDispatch owned by
    // `v`; `from_raw_borrowed` borrows it and `cloned` takes our own reference.
    let borrowed = unsafe { IDispatch::from_raw_borrowed(&ptr) };
    borrowed
        .cloned()
        .ok_or_else(|| OmniError::Com(format!("{what} came back as a null object")))
}

impl OmniRigCom {
    /// Create the OmniRig server object and take hold of `slot`'s rig object.
    ///
    /// **Runs on the worker thread** — this is where the apartment is entered.
    pub fn connect(slot: RigSlot) -> Result<Self, OmniError> {
        let progid = wide(PROGID);
        // SAFETY: a fresh worker thread, so no prior apartment can conflict. S_FALSE means
        // "already initialised on this thread" and still requires a matching CoUninitialize.
        let hr = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
        if hr.is_err() {
            return Err(OmniError::Com(format!(
                "could not enter a COM apartment: {} (0x{:08X})",
                hr.message(),
                hr.0
            )));
        }
        let apartment = Apartment; // from here on, every early return uninitialises COM

        // SAFETY: `progid` is NUL-terminated and outlives the call.
        let clsid = unsafe { CLSIDFromProgID(PCWSTR(progid.as_ptr())) }.map_err(|e| {
            if e.code() == CO_E_CLASSSTRING || e.code() == REGDB_E_CLASSNOTREG {
                OmniError::NotInstalled
            } else {
                com_err("looking up OmniRig in the registry", &e)
            }
        })?;
        // SAFETY: `clsid` is a valid CLSID; the requested interface is IDispatch.
        let server: IDispatch =
            unsafe { CoCreateInstance(&clsid, None, CLSCTX_ALL) }.map_err(|e| {
                if e.code() == REGDB_E_CLASSNOTREG {
                    OmniError::NotInstalled
                } else if is_elevation_required(e.code()) {
                    // Reported by an operator 2026-08-20 as "are there missing settings?" —
                    // which is exactly how the raw HRESULT reads. There is no setting; the
                    // registered server wants a higher integrity level than this process has.
                    OmniError::NeedsElevation
                } else {
                    com_err("starting OmniRig", &e)
                }
            })?;

        let slot_id = dispid(&server, slot.property())?;
        let rig = as_dispatch(
            &get_prop(&server, slot_id, slot.property())?,
            slot.property(),
        )?;
        let ids = RigIds {
            status: dispid(&rig, "Status")?,
            status_str: dispid(&rig, "StatusStr")?,
            freq: dispid(&rig, "Freq")?,
            mode: dispid(&rig, "Mode")?,
            tx: dispid(&rig, "Tx")?,
            split: dispid(&rig, "Split")?,
        };
        Ok(OmniRigCom {
            _server: server,
            rig,
            ids,
            slot,
            _apartment: apartment,
        })
    }

    fn get_i32(&self, id: i32, what: &str) -> Result<i32, OmniError> {
        as_i32(&get_prop(&self.rig, id, what)?, what)
    }
}

impl OmniRigClient for OmniRigCom {
    fn status(&self) -> Result<(OmniStatus, String), OmniError> {
        let code = self.get_i32(self.ids.status, "the rig status")?;
        // OmniRig's own sentence is what the operator should read; ours is only the fallback
        // when it is empty (see `OmniStatus::describe`).
        let text = match get_prop(&self.rig, self.ids.status_str, "the rig status text") {
            Ok(v) => BSTR::try_from(&v)
                .map(|b| b.to_string())
                .unwrap_or_default(),
            Err(_) => String::new(),
        };
        let _ = self.slot;
        Ok((OmniStatus::from_code(code), text))
    }

    fn freq_hz(&self) -> Result<u64, OmniError> {
        let hz = self.get_i32(self.ids.freq, "the dial frequency")?;
        // OmniRig reports 0 before the rig has answered; negative is not a frequency.
        Ok(hz.max(0) as u64)
    }

    fn set_freq_hz(&self, hz: u64) -> Result<(), OmniError> {
        // OmniRig's `Freq` is a 32-bit signed integer of Hz, so ~2.147 GHz is its ceiling —
        // above every amateur allocation it can drive. Refuse rather than wrap.
        let v = i32::try_from(hz).map_err(|_| {
            OmniError::Com(format!(
                "{hz} Hz is outside the range OmniRig can carry (its dial is a 32-bit Hz value)"
            ))
        })?;
        put_prop_i32(&self.rig, self.ids.freq, v, "the dial frequency")
    }

    fn mode(&self) -> Result<Option<OmniMode>, OmniError> {
        Ok(OmniMode::from_param(
            self.get_i32(self.ids.mode, "the mode")?,
        ))
    }

    fn set_mode(&self, m: OmniMode) -> Result<(), OmniError> {
        put_prop_i32(&self.rig, self.ids.mode, m.param(), "the mode")
    }

    fn ptt(&self) -> Result<bool, OmniError> {
        Ok(self.get_i32(self.ids.tx, "the PTT state")? == param::PM_TX)
    }

    fn set_ptt(&self, on: bool) -> Result<(), OmniError> {
        let v = if on { param::PM_TX } else { param::PM_RX };
        put_prop_i32(&self.rig, self.ids.tx, v, "PTT")
    }

    fn split(&self) -> Result<bool, OmniError> {
        Ok(self.get_i32(self.ids.split, "the split state")? == param::PM_SPLITON)
    }

    fn set_split(&self, on: bool) -> Result<(), OmniError> {
        let v = if on {
            param::PM_SPLITON
        } else {
            param::PM_SPLITOFF
        };
        put_prop_i32(&self.rig, self.ids.split, v, "split")
    }
}
