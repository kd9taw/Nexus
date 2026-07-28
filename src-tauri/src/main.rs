// Tempo desktop binary entry point.
//
// `windows_subsystem = "windows"` (release only) prevents a console window from
// flashing behind the GUI on Windows. All real logic lives in the library
// (`tempo_lib::run`) so it can be unit-tested and reused on mobile targets.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// Native-crash reporter (Windows).
//
// Nexus is a Rust GUI over ~40 vendored Fortran DSP cores. A bug in that layer
// surfaces as an ACCESS VIOLATION (0xC0000005), not a Rust panic: `catch_unwind`
// cannot contain it, no backtrace is printed, and the window simply vanishes.
// All the operator can report is "it closed", and all Windows Error Reporting
// records is a fault offset into a stripped binary.
//
// Earning this module: the JT65 Call-CQ crash in 0.19.16 took a multi-day hunt
// that a single stack would have ended immediately (it was `sync65.f90` indexing
// `ccfblue(lagpk)` with an uninitialized `lagpk`). The report below names the
// faulting MODULE, which is the question that matters first — Nexus itself, the
// Fortran runtime, WebView2, or Windows.
//
// It installs a vectored exception handler that fires FIRST-CHANCE, before the
// process dies, and writes `nexus-crash.txt` next to the executable. It always
// returns EXCEPTION_CONTINUE_SEARCH, so it only observes — the crash proceeds
// exactly as it would without it.
//
// Constraints that shape the code below:
//   • No allocation. A fault caused by a corrupted heap would deadlock or
//     re-fault inside the allocator, and we would get nothing. Everything is
//     formatted by hand into one fixed static buffer and written with WriteFile.
//   • Small stack footprint. A stack-overflow guard-page hit is itself an access
//     violation, and the handler then runs with about one page of headroom.
//   • No new dependencies: the Win32 calls are declared inline.
#[cfg(windows)]
mod crashlog {
    use std::ffi::c_void;
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};

    const EXCEPTION_ACCESS_VIOLATION: u32 = 0xC000_0005;
    const CONTINUE_SEARCH: i32 = 0;
    const FROM_ADDRESS_UNCHANGED: u32 = 0x0000_0004 | 0x0000_0002;
    const GENERIC_WRITE: u32 = 0x4000_0000;
    const CREATE_ALWAYS: u32 = 2;
    const MAX_FRAMES: usize = 48;
    const OUT_CAP: usize = 8192;

    #[repr(C)]
    struct ExceptionRecord {
        code: u32,
        flags: u32,
        next: *mut ExceptionRecord,
        address: *mut c_void,
        n_params: u32,
        params: [usize; 15],
    }

    #[repr(C)]
    struct ExceptionPointers {
        record: *mut ExceptionRecord,
        context: *mut c_void,
    }

    extern "system" {
        fn AddVectoredExceptionHandler(
            first: u32,
            handler: unsafe extern "system" fn(*mut ExceptionPointers) -> i32,
        ) -> *mut c_void;
        fn RtlCaptureStackBackTrace(
            skip: u32,
            capture: u32,
            frames: *mut *mut c_void,
            hash: *mut u32,
        ) -> u16;
        fn GetModuleHandleExW(flags: u32, addr: *const c_void, module: *mut *mut c_void) -> i32;
        fn GetModuleFileNameW(module: *mut c_void, buf: *mut u16, size: u32) -> u32;
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            sa: *mut c_void,
            disposition: u32,
            flags: u32,
            template: *mut c_void,
        ) -> *mut c_void;
        fn WriteFile(
            file: *mut c_void,
            buf: *const u8,
            len: u32,
            written: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
        fn CloseHandle(handle: *mut c_void) -> i32;
        fn GetCurrentThreadId() -> u32;
        fn GetTempPathW(len: u32, buf: *mut u16) -> u32;
    }

    /// The report, built in place. Static so the handler needs almost no stack.
    static mut OUT: [u8; OUT_CAP] = [0; OUT_CAP];
    static mut OUT_LEN: usize = 0;
    static mut FRAMES: [*mut c_void; MAX_FRAMES] = [ptr::null_mut(); MAX_FRAMES];
    static FIRED: AtomicBool = AtomicBool::new(false);

    /// Append raw bytes to the report, truncating rather than overflowing.
    unsafe fn put(bytes: &[u8]) {
        let out = ptr::addr_of_mut!(OUT) as *mut u8;
        let len = ptr::addr_of_mut!(OUT_LEN);
        for &b in bytes {
            if *len >= OUT_CAP {
                return;
            }
            *out.add(*len) = b;
            *len += 1;
        }
    }

    /// Append `value` as fixed-width `0x…` hex — no allocation, no core::fmt.
    unsafe fn put_hex(value: usize, digits: usize) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        put(b"0x");
        let mut i = digits;
        while i > 0 {
            i -= 1;
            put(&[HEX[(value >> (i * 4)) & 0xF]]);
        }
    }

    /// Append a decimal number.
    unsafe fn put_dec(mut value: u32) {
        let mut digits = [0u8; 10];
        let mut n = 0;
        loop {
            digits[n] = b'0' + (value % 10) as u8;
            value /= 10;
            n += 1;
            if value == 0 {
                break;
            }
        }
        while n > 0 {
            n -= 1;
            put(&[digits[n]]);
        }
    }

    /// Resolve `addr` to its owning module and append `name+0xoffset`.
    ///
    /// The module is the point of the whole exercise: it says whether the fault
    /// is in Nexus itself, in the vendored Fortran's runtime (libgfortran), in
    /// WebView2, or in Windows.
    unsafe fn put_module_relative(addr: *mut c_void) {
        let mut module: *mut c_void = ptr::null_mut();
        if GetModuleHandleExW(FROM_ADDRESS_UNCHANGED, addr, &mut module) == 0 || module.is_null() {
            put(b"<unknown module> ");
            put_hex(addr as usize, 16);
            return;
        }
        let mut path = [0u16; 260];
        let n = GetModuleFileNameW(module, path.as_mut_ptr(), 260) as usize;
        // Basename only: everything after the last backslash.
        let mut start = 0;
        for i in 0..n {
            if path[i] == b'\\' as u16 {
                start = i + 1;
            }
        }
        for &wide in path.iter().take(n).skip(start) {
            // Module file names are ASCII in practice; the low byte is the char.
            put(&[(wide & 0xFF) as u8]);
        }
        put(b"+");
        put_hex(addr as usize - module as usize, 8);
    }

    /// Append `nexus-crash.txt\0` at `cut` and try to write the report there.
    /// Returns false if the file could not be created.
    unsafe fn try_write(path: &mut [u16; 260], cut: usize) -> bool {
        for (i, ch) in b"nexus-crash.txt\0".iter().enumerate() {
            if cut + i >= 260 {
                return false;
            }
            path[cut + i] = *ch as u16;
        }
        let file = CreateFileW(
            path.as_ptr(),
            GENERIC_WRITE,
            0,
            ptr::null_mut(),
            CREATE_ALWAYS,
            0,
            ptr::null_mut(),
        );
        if file as isize == -1 {
            return false;
        }
        let mut written = 0u32;
        WriteFile(
            file,
            ptr::addr_of!(OUT) as *const u8,
            OUT_LEN as u32,
            &mut written,
            ptr::null_mut(),
        );
        CloseHandle(file);
        true
    }

    /// Write the report beside the executable, falling back to `%TEMP%`.
    ///
    /// The fallback is the case that actually happens: the installer puts Nexus
    /// under Program Files, which a normal user cannot write to, so writing only
    /// beside the executable would silently produce nothing.
    unsafe fn flush() {
        let mut path = [0u16; 260];
        let n = GetModuleFileNameW(ptr::null_mut(), path.as_mut_ptr(), 260) as usize;
        if n > 0 {
            let mut cut = 0;
            for i in 0..n {
                if path[i] == b'\\' as u16 {
                    cut = i + 1;
                }
            }
            if try_write(&mut path, cut) {
                return;
            }
        }
        // GetTempPathW includes the trailing backslash, so its length IS the cut.
        let mut temp = [0u16; 260];
        let n = GetTempPathW(260, temp.as_mut_ptr()) as usize;
        if n > 0 && n < 240 {
            try_write(&mut temp, n);
        }
    }

    unsafe extern "system" fn handler(info: *mut ExceptionPointers) -> i32 {
        if info.is_null() {
            return CONTINUE_SEARCH;
        }
        let record = (*info).record;
        if record.is_null() || (*record).code != EXCEPTION_ACCESS_VIOLATION {
            return CONTINUE_SEARCH;
        }
        // Report the FIRST access violation only. Later ones are usually the
        // unwinder tripping over the same broken state.
        if FIRED.swap(true, Ordering::SeqCst) {
            return CONTINUE_SEARCH;
        }

        put(b"Nexus native-crash report (see CHANGELOG for how to send this in)\r\n");
        put(b"exception : ACCESS_VIOLATION (0xc0000005)\r\n");
        put(b"faulting  : ");
        put_module_relative((*record).address);
        put(b"\r\nthread    : ");
        put_dec(GetCurrentThreadId());

        // ExceptionInformation distinguishes a null deref from a wild pointer
        // from a guard-page hit (stack overflow) — different bugs entirely.
        if (*record).n_params >= 2 {
            put(b"\r\noperation : ");
            put(match (*record).params[0] {
                0 => b"READ from  ".as_slice(),
                1 => b"WRITE to   ".as_slice(),
                8 => b"EXECUTE at ".as_slice(),
                _ => b"?? at      ".as_slice(),
            });
            put_hex((*record).params[1], 16);
        }

        put(b"\r\n\r\nstack (innermost first):\r\n");
        let frames = ptr::addr_of_mut!(FRAMES) as *mut *mut c_void;
        let n = RtlCaptureStackBackTrace(0, MAX_FRAMES as u32, frames, ptr::null_mut()) as usize;
        for i in 0..n {
            let frame = *frames.add(i);
            if frame.is_null() {
                continue;
            }
            put(b"  ");
            put_module_relative(frame);
            put(b"\r\n");
        }
        if n == 0 {
            put(b"  <no frames captured>\r\n");
        }

        flush();
        CONTINUE_SEARCH
    }

    /// Install the handler. Call once, as early as possible.
    pub fn install() {
        unsafe {
            AddVectoredExceptionHandler(1, handler);
        }
    }
}

fn main() {
    #[cfg(windows)]
    crashlog::install();

    tempo_lib::run();
}
