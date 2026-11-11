//! The only unsafe code of the tray: the Win32 calls the tray and menu
//! crates do not wrap. Each block is a single FFI call on arguments built
//! right above it; no pointer outlives the call.
#![allow(unsafe_code)] // FFI into user32/kernel32/advapi32; nothing here has a safe binding

use std::ptr::{null, null_mut};
use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, ERROR_SUCCESS};
use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows_sys::Win32::System::Threading::{CreateMutexW, GetCurrentThreadId};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, PostThreadMessageW, TranslateMessage, MSG, WM_APP, WM_CLOSE,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Holds the named mutex for the life of the process.
#[derive(Debug)]
pub struct Instance(#[allow(dead_code)] isize);

/// Takes the mutex `name`; `None` when another process already holds it.
pub fn single_instance(name: &str) -> Option<Instance> {
    let name = wide(name);
    // SAFETY: `name` is NUL-terminated and outlives the call; no security
    // attributes, not initially owned.
    let (handle, err) = unsafe {
        let h = CreateMutexW(null(), 0, name.as_ptr());
        (h, GetLastError())
    };
    if handle.is_null() || err == ERROR_ALREADY_EXISTS {
        return None;
    }
    Some(Instance(handle as isize))
}

/// Id of the calling thread; workers pass it to [`wake`].
pub fn thread_id() -> u32 {
    // SAFETY: no arguments, no failure mode.
    unsafe { GetCurrentThreadId() }
}

/// Wakes the pump of thread `tid`. Never blocks; a lost wake is retried by
/// the next tick.
pub fn wake(tid: u32) {
    // SAFETY: posts a plain message with no pointer payload.
    unsafe {
        PostThreadMessageW(tid, WM_APP, 0, 0);
    }
}

/// The message pump: dispatches window messages, calls `on_wake` for every
/// [`wake`] and stops when it returns `false`, the queue closes or a
/// window of the tray is asked to close (`taskkill` without `/F`, as
/// `bar tray stop` sends it): returning lets the icon be removed instead
/// of lingering after a forced end.
pub fn pump(mut on_wake: impl FnMut() -> bool) {
    // SAFETY: `msg` is zeroed plain data the calls fill in and read back on
    // this thread only.
    unsafe {
        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, null_mut(), 0, 0) > 0 {
            if msg.hwnd.is_null() && msg.message == WM_APP {
                if !on_wake() {
                    break;
                }
                continue;
            }
            if msg.message == WM_CLOSE {
                break;
            }
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// Whether the taskbar uses the light theme; `None` when unreadable.
pub fn taskbar_is_light() -> Option<bool> {
    let key = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let value = wide("SystemUsesLightTheme");
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: both names are NUL-terminated; `data`/`size` describe one
    // DWORD the call writes into.
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            null_mut(),
            (&raw mut data).cast(),
            &mut size,
        )
    };
    (rc == ERROR_SUCCESS).then_some(data != 0)
}
