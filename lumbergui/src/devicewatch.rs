//! Hear from Windows when something is plugged in or unplugged.
//!
//! Checking a rig means opening it, which is slow and — for a serial port —
//! resets the board on the other end. So it is done when somebody asks, and
//! otherwise only when there is reason to think the answer has changed. This
//! is that reason: the operating system already knows, and will say so.
//!
//! The signal carries no detail worth reading. Windows names the device
//! interface that came or went in terms that have nothing to do with what a
//! rig calls its devices, so this reports only *that* something changed and
//! lets the interface ask the rig itself.
//!
//! # Why a window
//!
//! Device notifications are delivered as window messages, and iced's window
//! belongs to winit, whose procedure we cannot hook. So this owns a window of
//! its own — invisible, message-only, on a thread with nothing else to do.
//!
//! One detail decides whether any of this works: `DBT_DEVNODES_CHANGED` is
//! broadcast to *top level* windows, and a message-only window never receives
//! it. What does arrive at a message-only window is a targeted registration,
//! which is why this registers with `DEVICE_NOTIFY_ALL_INTERFACE_CLASSES`
//! rather than listening for the broadcast. Getting that wrong gives silence
//! rather than an error, which is the worst way to be wrong.

use iced::futures::channel::mpsc::Sender;

/// Watch for devices arriving and leaving, reporting each as one message.
///
/// The stream is what carries this to the interface. A plain channel would
/// not: the window stops asking to be drawn when nothing is happening, and a
/// message sitting unread in a channel is exactly the case where nothing is
/// happening. A subscription wakes it.
pub(crate) fn changes() -> impl iced::futures::Stream<Item = crate::Message> {
    iced::stream::channel(4, |sender| async move {
        // The watching is blocking — a Win32 message loop — so it belongs on
        // a thread of its own rather than on the executor.
        std::thread::spawn(move || watch(sender));

        // Nothing further to do here, but the stream lives as long as this
        // does, so it waits rather than returning.
        std::future::pending::<()>().await;
    })
}

#[cfg(windows)]
fn watch(sender: Sender<crate::Message>) {
    use std::ffi::c_void;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, RegisterClassW,
        RegisterDeviceNotificationW, DBT_DEVICEARRIVAL, DBT_DEVICEREMOVECOMPLETE,
        DBT_DEVTYP_DEVICEINTERFACE, DEVICE_NOTIFY_ALL_INTERFACE_CLASSES,
        DEVICE_NOTIFY_WINDOW_HANDLE, DEV_BROADCAST_DEVICEINTERFACE_W, HWND_MESSAGE, MSG,
        WM_DEVICECHANGE, WNDCLASSW,
    };

    // The window procedure is a bare function with nowhere to carry state, and
    // everything here happens on this one thread, so the sender waits where
    // that function can reach it.
    thread_local! {
        static REPORT: std::cell::RefCell<Option<Sender<crate::Message>>> =
            const { std::cell::RefCell::new(None) };
    }

    unsafe extern "system" fn procedure(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if message == WM_DEVICECHANGE {
            let event = wparam as u32;
            if event == DBT_DEVICEARRIVAL || event == DBT_DEVICEREMOVECOMPLETE {
                REPORT.with(|report| {
                    if let Some(sender) = report.borrow_mut().as_mut() {
                        // Dropped rather than queued if the interface is busy:
                        // this says "something changed", and two of those say
                        // no more than one.
                        let _ = sender.try_send(crate::Message::DevicesChanged);
                    }
                });
            }
        }

        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }

    REPORT.with(|report| *report.borrow_mut() = Some(sender));

    // Nul terminated, as every Win32 string must be.
    let class: Vec<u16> = "LumberjackDeviceWatch\0".encode_utf16().collect();

    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());

        let mut definition: WNDCLASSW = std::mem::zeroed();
        definition.lpfnWndProc = Some(procedure);
        definition.hInstance = instance as _;
        definition.lpszClassName = class.as_ptr();

        if RegisterClassW(&definition) == 0 {
            return;
        }

        // `HWND_MESSAGE` as the parent is what makes this a message-only
        // window: no pixels, no taskbar, nothing to see.
        let window = CreateWindowExW(
            0,
            class.as_ptr(),
            std::ptr::null(),
            0,
            0,
            0,
            0,
            0,
            HWND_MESSAGE,
            std::ptr::null_mut(),
            instance as _,
            std::ptr::null(),
        );

        if window.is_null() {
            return;
        }

        let mut filter: DEV_BROADCAST_DEVICEINTERFACE_W = std::mem::zeroed();
        filter.dbcc_size = std::mem::size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32;
        filter.dbcc_devicetype = DBT_DEVTYP_DEVICEINTERFACE;
        // The class guid is left zeroed: with every interface class asked for,
        // it is not read.

        let registration = RegisterDeviceNotificationW(
            window as _,
            &filter as *const _ as *const c_void,
            DEVICE_NOTIFY_WINDOW_HANDLE | DEVICE_NOTIFY_ALL_INTERFACE_CLASSES,
        );

        if registration.is_null() {
            return;
        }

        // Messages arrive here and are handed to the procedure above. This
        // thread does nothing else, so blocking in `GetMessageW` costs
        // nothing and wakes only when Windows has something to say.
        let mut message: MSG = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            DispatchMessageW(&message);
        }
    }
}

/// Everywhere else, there is nothing to listen to.
///
/// Linux would be udev and macOS IOKit, neither of which is written here. The
/// interface loses nothing it had: checking on opening a project, on stopping
/// a run and on being asked all still work.
#[cfg(not(windows))]
fn watch(_sender: Sender<crate::Message>) {}
