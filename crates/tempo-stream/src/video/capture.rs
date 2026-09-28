//! Window-only capture (S1, security test A1): the station's own Nexus window, and nothing else,
//! through Windows Graphics Capture (windows-capture).
//!
//! - **One source, by construction.** The capture item is built from the window handle the
//!   application resolved for its `main` window, with `IGraphicsCaptureItemInterop::CreateForWindow`
//!   (windows-capture's `Window`). Nothing in Nexus builds a monitor item or a desktop duplication,
//!   so another application's window, even one lying over Nexus, is not in the picture: WGC
//!   composes the captured window on its own. Owned windows outside it (a file picker) are left out
//!   too, WGC's default, which is kept.
//! - **The client area only.** WGC captures the window's frame, title bar included; the picture is
//!   cropped to the client area, so the page's input coordinates (fractions of the picture) are
//!   the webview's own. Measured in physical pixels on a per-monitor-aware thread, as WGC's frames
//!   are.
//! - **Nothing while the window cannot be seen.** A minimized or hidden window contributes no
//!   picture, and [`showing`] tells the encoder not to send the last one again.
//! - **Every picture is kept until the next.** The capture never skips a delivered frame, because
//!   WGC delivers one only when the window changes: a skipped frame could be the window's final
//!   state, and the page would be shown the one before it as current. The encoder's one-picture
//!   mailbox is where frames are dropped, and only in favour of a newer one.
//! - **Below the radio**, like the encoder: the capture thread lowers its own priority.
//! - **WGC's yellow border stays**, drawn round the window at the station while it is captured.
use std::ffi::c_void;
use std::sync::Arc;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::System::Threading::{
    GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL,
};
use windows::Win32::UI::HiDpi::{
    SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{GetClientRect, IsIconic, IsWindow, IsWindowVisible};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl};
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings,
    MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings,
};
use windows_capture::window::Window;

use super::picture::{Bgra, MIN_SIDE};
use super::pipeline::{Mailbox, FRAME_INTERVAL};

/// Why a capture could not start.
#[derive(Debug)]
pub struct CaptureError(pub String);

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Drop the calling thread below normal priority: the radio loop runs at normal.
pub fn lower_priority() {
    // SAFETY: the pseudo-handle of the calling thread; failure leaves the priority as it was.
    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
}

fn hwnd(window: isize) -> HWND {
    HWND(window as *mut c_void)
}

/// Can the window be seen: still a window, visible, and not minimized?
pub fn showing(window: isize) -> bool {
    let hwnd = hwnd(window);
    // SAFETY: these only read the window's state; a stale handle reads as not a window.
    unsafe {
        IsWindow(Some(hwnd)).as_bool()
            && IsWindowVisible(hwnd).as_bool()
            && !IsIconic(hwnd).as_bool()
    }
}

/// Can this station capture `window` at all: Windows Graphics Capture present, and a real window?
pub fn available(window: isize) -> bool {
    // SAFETY: as in `showing`.
    let is_window = unsafe { IsWindow(Some(hwnd(window))).as_bool() };
    is_window && GraphicsCaptureApi::is_supported().unwrap_or(false)
}

/// Where the client area sits in a captured frame of `frame_w`×`frame_h`: its left, top, width and
/// height in the frame's pixels. `None` if there is not enough of it to encode.
fn client_area(window: isize, frame_w: u32, frame_h: u32) -> Option<(u32, u32, u32, u32)> {
    let hwnd = hwnd(window);
    let mut bounds = RECT::default();
    let mut client = RECT::default();
    let mut origin = POINT::default();
    // SAFETY: each call writes only into the local it is given, sized as the API requires.
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            (&mut bounds as *mut RECT).cast::<c_void>(),
            std::mem::size_of::<RECT>() as u32,
        )
        .ok()?;
        GetClientRect(hwnd, &mut client).ok()?;
        if !ClientToScreen(hwnd, &mut origin).as_bool() {
            return None;
        }
    }
    let x = (origin.x - bounds.left).max(0) as u32;
    let y = (origin.y - bounds.top).max(0) as u32;
    // The frame can lag the window during a resize: never read past it.
    let w = ((client.right - client.left).max(0) as u32).min(frame_w.saturating_sub(x));
    let h = ((client.bottom - client.top).max(0) as u32).min(frame_h.saturating_sub(y));
    (w >= MIN_SIDE && h >= MIN_SIDE).then_some((x, y, w, h))
}

/// The capture thread's handler: every frame of the window, cropped and copied to the mailbox.
struct Handler {
    window: isize,
    mailbox: Arc<Mailbox>,
    started: bool,
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = (isize, Arc<Mailbox>);
    type Error = CaptureError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        let (window, mailbox) = ctx.flags;
        Ok(Self {
            window,
            mailbox,
            started: false,
        })
    }

    fn on_frame_arrived(
        &mut self,
        frame: &mut Frame,
        control: InternalCaptureControl,
    ) -> Result<(), Self::Error> {
        if !self.started {
            self.started = true;
            lower_priority();
            // Physical pixels for the crop, as WGC's frames are, whatever the process declared.
            // SAFETY: changes only this thread's own DPI context.
            unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) };
        }
        // The encoder is gone: stop capturing.
        if self.mailbox.ended() {
            control.stop();
            return Ok(());
        }
        if !showing(self.window) {
            return Ok(());
        }
        // A window too small to encode, or a frame that cannot be read, is a change the page
        // cannot be shown: the encoder is told the picture it holds is no longer the window.
        let Some((x, y, w, h)) = client_area(self.window, frame.width(), frame.height()) else {
            self.mailbox.lost();
            return Ok(());
        };
        let captured_at = Instant::now();
        let Ok(mut buffer) = frame.buffer_crop(x, y, x + w, y + h) else {
            self.mailbox.lost();
            return Ok(());
        };
        let pitch = buffer.row_pitch() as usize;
        let row = w as usize * 4;
        let raw = buffer.as_raw_buffer();
        if pitch < row || raw.len() < pitch * (h as usize - 1) + row {
            self.mailbox.lost();
            return Ok(());
        }
        let mut pixels = Vec::with_capacity(row * h as usize);
        for line in 0..h as usize {
            pixels.extend_from_slice(&raw[line * pitch..line * pitch + row]);
        }
        self.mailbox.put(Bgra {
            width: w,
            height: h,
            stride: row,
            pixels,
            captured_at,
        });
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        // The window is gone, and with it the stream's picture.
        self.mailbox.end();
        Ok(())
    }
}

/// A running capture of one window. Dropping it stops the capture thread.
pub struct Capture {
    control: Option<CaptureControl<Handler, CaptureError>>,
}

impl Capture {
    /// Capture `window` into `mailbox`, on a thread of its own.
    pub fn start(window: isize, mailbox: Arc<Mailbox>) -> Result<Self, CaptureError> {
        if !available(window) {
            return Err(CaptureError("window capture is not available".into()));
        }
        // The station's own cursor is not the remote operator's: leave it out where Windows lets
        // us choose.
        let cursor = if GraphicsCaptureApi::is_cursor_settings_supported().unwrap_or(false) {
            CursorCaptureSettings::WithoutCursor
        } else {
            CursorCaptureSettings::Default
        };
        // No more frames than the encoder can use, where Windows can throttle them itself.
        let interval =
            if GraphicsCaptureApi::is_minimum_update_interval_supported().unwrap_or(false) {
                MinimumUpdateIntervalSettings::Custom(FRAME_INTERVAL)
            } else {
                MinimumUpdateIntervalSettings::Default
            };
        let settings = Settings::new(
            Window::from_raw_hwnd(hwnd(window).0),
            cursor,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            interval,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            (window, mailbox),
        );
        let control = Handler::start_free_threaded(settings)
            .map_err(|error| CaptureError(error.to_string()))?;
        Ok(Self {
            control: Some(control),
        })
    }

    /// Has the capture ended on its own (the window closed, or WGC stopped)?
    pub fn ended(&self) -> bool {
        self.control
            .as_ref()
            .is_none_or(CaptureControl::is_finished)
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if let Some(control) = self.control.take() {
            let _ = control.stop();
        }
    }
}

#[cfg(test)]
mod tests {
    //! A1 against a real desktop: another window lying over the Nexus window is not in the
    //! picture, and the picture is the client area. `#[ignore]`d because it opens two small
    //! windows on the desktop it runs on (without taking focus), which a build machine does not
    //! have; run it on Windows with `--ignored`.
    use super::*;
    use std::time::Duration;
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::{COLORREF, HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::Graphics::Gdi::{
        CreateSolidBrush, GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetWindowRect,
        PeekMessageW, RegisterClassW, ShowWindow, TranslateMessage, MSG, PM_REMOVE,
        SW_SHOWNOACTIVATE, WINDOW_EX_STYLE, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        WS_EX_TOPMOST, WS_OVERLAPPEDWINDOW, WS_POPUP,
    };
    use windows_capture::monitor::Monitor;

    use crate::video::pipeline::Taken;

    extern "system" fn procedure(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        // SAFETY: the default procedure for our own test windows.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    /// A window of one solid colour (0x00BBGGRR).
    unsafe fn solid_window(
        class: PCWSTR,
        colour: u32,
        ex: WINDOW_EX_STYLE,
        style: windows::Win32::UI::WindowsAndMessaging::WINDOW_STYLE,
        (x, y, w, h): (i32, i32, i32, i32),
    ) -> HWND {
        let instance: HINSTANCE = GetModuleHandleW(None).unwrap().into();
        let wc = WNDCLASSW {
            lpfnWndProc: Some(procedure),
            hInstance: instance,
            hbrBackground: CreateSolidBrush(COLORREF(colour)),
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            ex,
            class,
            w!("Nexus A1 test"),
            style,
            x,
            y,
            w,
            h,
            None,
            None,
            Some(instance),
            None,
        )
        .unwrap();
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        hwnd
    }

    fn pump() {
        let mut msg = MSG::default();
        // SAFETY: this thread's own message queue.
        unsafe {
            while PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }

    /// The first picture in `mailbox`, pumping this thread's windows while waiting.
    fn first_picture(mailbox: &Mailbox) -> Bgra {
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            pump();
            if let Taken::Picture(picture) = mailbox.take(Duration::from_millis(20)) {
                return picture;
            }
        }
        panic!("no picture within five seconds");
    }

    fn pixel(picture: &Bgra, x: u32, y: u32) -> [u8; 4] {
        let at = y as usize * picture.stride + x as usize * 4;
        picture.pixels[at..at + 4].try_into().unwrap()
    }

    fn near(got: [u8; 4], want: [u8; 3]) -> bool {
        got[..3].iter().zip(want).all(|(g, w)| g.abs_diff(w) <= 8)
    }

    /// The whole of a monitor, uncropped: the control's screen capture.
    struct Whole(Arc<Mailbox>);

    impl GraphicsCaptureApiHandler for Whole {
        type Flags = Arc<Mailbox>;
        type Error = CaptureError;

        fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
            Ok(Self(ctx.flags))
        }

        fn on_frame_arrived(
            &mut self,
            frame: &mut Frame,
            _: InternalCaptureControl,
        ) -> Result<(), Self::Error> {
            let (w, h) = (frame.width(), frame.height());
            let mut buffer = frame.buffer().map_err(|e| CaptureError(e.to_string()))?;
            let pitch = buffer.row_pitch() as usize;
            let raw = buffer.as_raw_buffer();
            let row = w as usize * 4;
            let mut pixels = Vec::with_capacity(row * h as usize);
            for line in 0..h as usize {
                pixels.extend_from_slice(&raw[line * pitch..line * pitch + row]);
            }
            self.0.put(Bgra {
                width: w,
                height: h,
                stride: row,
                pixels,
                captured_at: Instant::now(),
            });
            Ok(())
        }
    }

    /// ★ A1. The "Nexus" window is red, with a title bar; a green window lies over its middle,
    /// topmost. The picture is the red window's client area, red all over, green nowhere.
    /// CONTROL: a capture of the monitor, in the same harness, shows the green window exactly
    /// there. A capture that took the screen instead of the window would fail this.
    #[test]
    #[ignore = "opens two windows on an interactive Windows desktop"]
    fn a_window_over_nexus_is_not_in_the_picture() {
        const RED: u32 = 0x0000_00FF;
        const GREEN: u32 = 0x0000_FF00;
        // SAFETY: test windows on this thread, destroyed before the test returns.
        let (nexus, foreign) = unsafe {
            SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let nexus = solid_window(
                w!("NexusA1Red"),
                RED,
                WINDOW_EX_STYLE(0),
                WS_OVERLAPPEDWINDOW,
                (160, 160, 480, 320),
            );
            let foreign = solid_window(
                w!("NexusA1Green"),
                GREEN,
                WS_EX_TOPMOST | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW,
                WS_POPUP,
                (320, 240, 160, 120),
            );
            (nexus, foreign)
        };
        for _ in 0..20 {
            pump();
            std::thread::sleep(Duration::from_millis(25));
        }
        let window = nexus.0 as isize;
        let mut client = RECT::default();
        let mut over = RECT::default();
        // SAFETY: reads of our own windows.
        unsafe {
            GetClientRect(nexus, &mut client).unwrap();
            GetWindowRect(foreign, &mut over).unwrap();
        }

        let mailbox = Arc::new(Mailbox::default());
        let capture = Capture::start(window, mailbox.clone()).expect("capture");
        let picture = first_picture(&mailbox);
        drop(capture);
        // The client area, not the frame: its size, and red from the very first row.
        assert_eq!(
            (picture.width, picture.height),
            (client.right as u32, client.bottom as u32)
        );
        let red = [0, 0, 255];
        assert!(
            near(pixel(&picture, 0, 0), red),
            "{:?}",
            pixel(&picture, 0, 0)
        );
        // Where the green window lies over it, the picture is still red.
        let mut origin = POINT::default();
        // SAFETY: a read of our own window.
        assert!(unsafe { ClientToScreen(nexus, &mut origin) }.as_bool());
        let (cx, cy) = ((over.left + over.right) / 2, (over.top + over.bottom) / 2);
        let (px, py) = ((cx - origin.x) as u32, (cy - origin.y) as u32);
        assert!(
            near(pixel(&picture, px, py), red),
            "the foreign window is in the picture: {:?}",
            pixel(&picture, px, py)
        );
        let green = [0, 255, 0];
        let greens = picture
            .pixels
            .chunks_exact(4)
            .filter(|p| near([p[0], p[1], p[2], p[3]], green))
            .count();
        assert_eq!(greens, 0, "green pixels in the picture");

        // CONTROL: the monitor, captured the same way, shows the green window there.
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        // SAFETY: a read of the monitor our window is on.
        let hmonitor = unsafe { MonitorFromWindow(foreign, MONITOR_DEFAULTTONEAREST) };
        assert!(unsafe { GetMonitorInfoW(hmonitor, &mut info) }.as_bool());
        let screen = Arc::new(Mailbox::default());
        let settings = Settings::new(
            Monitor::from_raw_hmonitor(hmonitor.0),
            CursorCaptureSettings::Default,
            DrawBorderSettings::Default,
            SecondaryWindowSettings::Default,
            MinimumUpdateIntervalSettings::Default,
            DirtyRegionSettings::Default,
            ColorFormat::Bgra8,
            screen.clone(),
        );
        let control = Whole::start_free_threaded(settings).expect("monitor capture");
        let whole = first_picture(&screen);
        let _ = control.stop();
        let (sx, sy) = (
            (cx - info.rcMonitor.left) as u32,
            (cy - info.rcMonitor.top) as u32,
        );
        assert!(
            near(pixel(&whole, sx, sy), green),
            "control: the screen capture does not show the green window: {:?}",
            pixel(&whole, sx, sy)
        );

        // SAFETY: our own windows.
        unsafe {
            let _ = DestroyWindow(foreign);
            let _ = DestroyWindow(nexus);
        }
    }
}
