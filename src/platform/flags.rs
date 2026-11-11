//! Windows creation flags for what bar starts, as plain functions so
//! every OS tests them; only the Windows platform applies them.
//!
//! `CommandExt::creation_flags` replaces the flags set before it, it does
//! not add to them, so each child gets its whole set from one call
//! ([`job`], [`tool`], [`refresh`]); two helpers that set different sets on the
//! same command would leave only the last one.
//!
//! Consoles: a console child of a process with a console shares that
//! console unless told otherwise. `CREATE_NO_WINDOW` gives it a console
//! of its own without a window (a console host start per child), which
//! every descendant then shares. `DETACHED_PROCESS` is never used for
//! console children: a process without a console makes each console
//! child it starts without flags open a new, visible console window.
//! A child inherits this process's console only when that console is
//! known to be private: the parent created it hidden for this process
//! and said so in [`PRIVATE_CONSOLE_ENV`] (a background `bar refresh`,
//! `bar` started by the tray). A status line render may share the
//! console of the terminal running Claude Code, so its children always
//! get one of their own.

/// `CREATE_NEW_PROCESS_GROUP`: console control events of the parent's
/// group do not reach it.
pub const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
/// `CREATE_BREAKAWAY_FROM_JOB`: out of the caller's job object, when the
/// job allows it.
pub const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
/// `CREATE_NO_WINDOW`: a console of its own, without a window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// `DETACHED_PROCESS`: no console at all. Not used for console children.
pub const DETACHED_PROCESS: u32 = 0x0000_0008;
/// `ERROR_ACCESS_DENIED`: what process creation returns when the job the
/// caller is in refuses breakaway.
pub const ERROR_ACCESS_DENIED: i32 = 5;

/// Set to [`PRIVATE_CONSOLE`] by a parent that created this process's
/// console hidden, for this process alone. Removed from the children.
pub const PRIVATE_CONSOLE_ENV: &str = "BAR_PRIVATE_CONSOLE";
/// The one value of [`PRIVATE_CONSOLE_ENV`] that counts.
pub const PRIVATE_CONSOLE: &str = "1";

/// Whether this process's console is private: the parent marked it, and
/// there is a console (one that cannot be confirmed is not trusted).
pub fn private_console(marked: bool, has_console: bool) -> bool {
    marked && has_console
}

/// Whether the marker's value is [`PRIVATE_CONSOLE`].
pub fn marked(value: Option<&std::ffi::OsStr>) -> bool {
    value.is_some_and(|v| v == PRIVATE_CONSOLE)
}

/// A child bar waits for and may kill with its descendants (jobs,
/// plugins, `%command%`; `own_group`): its own process group; this
/// process's console when it is private, else a console of its own
/// without a window.
pub fn job(private: bool) -> u32 {
    if private {
        CREATE_NEW_PROCESS_GROUP
    } else {
        CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
    }
}

/// A tool bar waits for (`git` in a render, `reg`, `taskkill`;
/// `hide_window`): the same set as [`job`], so the two helpers compose
/// on one command in either order.
pub fn tool(private: bool) -> u32 {
    job(private)
}

/// A background `bar refresh`: a console of its own without a window,
/// which its children inherit; its own process group; out of the
/// caller's job object when `breakaway`.
pub fn refresh(breakaway: bool) -> u32 {
    let job = if breakaway {
        CREATE_BREAKAWAY_FROM_JOB
    } else {
        0
    };
    CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP | job
}

/// A spawn with `CREATE_BREAKAWAY_FROM_JOB` that failed with `code` is
/// tried again without it only when the job refused the breakaway.
pub fn retry_without_breakaway(code: Option<i32>) -> bool {
    code == Some(ERROR_ACCESS_DENIED)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    /// `creation_flags` semantics: the last call's set is what the child
    /// gets.
    fn last_wins(calls: &[u32]) -> u32 {
        calls.last().copied().unwrap_or(0)
    }

    #[test]
    fn helpers_on_one_command_compose_in_any_order() {
        for private in [false, true] {
            let (group, hide) = (job(private), tool(private));
            for calls in [[group, hide], [hide, group]] {
                assert_eq!(last_wins(&calls), group | hide, "private={private}");
            }
        }
    }

    #[test]
    fn children_always_lead_their_own_group() {
        for private in [false, true] {
            for f in [job(private), tool(private)] {
                assert_ne!(f & CREATE_NEW_PROCESS_GROUP, 0);
                assert_eq!(f & DETACHED_PROCESS, 0);
            }
        }
    }

    #[test]
    fn children_share_only_a_private_console() {
        for f in [job, tool] {
            assert_eq!(f(true) & CREATE_NO_WINDOW, 0);
            assert_ne!(f(false) & CREATE_NO_WINDOW, 0);
        }
    }

    #[test]
    fn a_refresh_gets_a_hidden_console_not_none() {
        for b in [false, true] {
            let f = refresh(b);
            assert_eq!(f & DETACHED_PROCESS, 0);
            assert_eq!(
                f & (CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP),
                CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP
            );
        }
        assert_ne!(refresh(true) & CREATE_BREAKAWAY_FROM_JOB, 0);
        assert_eq!(refresh(false) & CREATE_BREAKAWAY_FROM_JOB, 0);
    }

    #[test]
    fn only_a_refused_breakaway_is_retried() {
        assert!(retry_without_breakaway(Some(ERROR_ACCESS_DENIED)));
        assert!(!retry_without_breakaway(Some(2))); // not found
        assert!(!retry_without_breakaway(None));
    }

    #[test]
    fn a_console_is_private_only_when_marked_and_present() {
        assert!(private_console(true, true));
        assert!(!private_console(true, false));
        assert!(!private_console(false, true));
        assert!(!private_console(false, false));
    }

    #[test]
    fn only_the_exact_marker_counts() {
        assert!(marked(Some(OsStr::new("1"))));
        for v in ["", "0", "true", "1 "] {
            assert!(!marked(Some(OsStr::new(v))), "{v:?}");
        }
        assert!(!marked(None));
    }

    #[cfg(windows)]
    #[test]
    fn constants_match_the_windows_api() {
        use windows_sys::Win32::Foundation as F;
        use windows_sys::Win32::System::Threading as T;
        assert_eq!(CREATE_NEW_PROCESS_GROUP, T::CREATE_NEW_PROCESS_GROUP);
        assert_eq!(CREATE_BREAKAWAY_FROM_JOB, T::CREATE_BREAKAWAY_FROM_JOB);
        assert_eq!(CREATE_NO_WINDOW, T::CREATE_NO_WINDOW);
        assert_eq!(DETACHED_PROCESS, T::DETACHED_PROCESS);
        assert_eq!(ERROR_ACCESS_DENIED, F::ERROR_ACCESS_DENIED as i32);
    }
}
