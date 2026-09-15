//! Terminal color initialization and Windows console mode management.

#[cfg(windows)]
mod windows_console {
    use std::io::IsTerminal;

    const STD_OUTPUT_HANDLE: u32 = 0xFFFFFFF5; // ((DWORD)-11)
    const STD_ERROR_HANDLE: u32 = 0xFFFFFFF4; // ((DWORD)-12)
    const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> *mut std::ffi::c_void;
        fn GetConsoleMode(hConsoleHandle: *mut std::ffi::c_void, lpMode: *mut u32) -> i32;
        fn SetConsoleMode(hConsoleHandle: *mut std::ffi::c_void, dwMode: u32) -> i32;
    }

    pub fn setup_terminal_colors() {
        let stdout_is_term = std::io::stdout().is_terminal();
        let stderr_is_term = std::io::stderr().is_terminal();

        if !stdout_is_term && !stderr_is_term {
            return;
        }

        let enable_vt = |std_id: u32| -> bool {
            let handle = unsafe { GetStdHandle(std_id) };
            if handle.is_null() || handle == usize::MAX as *mut std::ffi::c_void {
                return false;
            }

            let mut mode: u32 = 0;
            if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
                return false;
            }

            if mode & ENABLE_VIRTUAL_TERMINAL_PROCESSING != 0 {
                return true;
            }

            unsafe { SetConsoleMode(handle, mode | ENABLE_VIRTUAL_TERMINAL_PROCESSING) != 0 }
        };

        let mut stdout_vt_ok = true;
        if stdout_is_term {
            stdout_vt_ok = enable_vt(STD_OUTPUT_HANDLE);
        }

        if stderr_is_term {
            let _ = enable_vt(STD_ERROR_HANDLE);
        }

        // If stdout is an interactive terminal but cannot activate VT processing
        // (such as legacy Windows consoles without VT support), gracefully disable
        // colored output to prevent raw escape sequences from leaking into the display.
        if stdout_is_term && !stdout_vt_ok {
            colored::control::set_override(false);
        }
    }
}

/// Initialize terminal ANSI color support across platforms.
/// On Windows, activates virtual terminal processing on stdout and stderr.
/// If virtual terminal processing is unsupported, gracefully falls back to plain text.
#[inline]
pub fn init_terminal_colors() {
    #[cfg(windows)]
    windows_console::setup_terminal_colors();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init_terminal_colors_idempotent() {
        // Calling init_terminal_colors multiple times should be safe and idempotent
        init_terminal_colors();
        init_terminal_colors();
    }
}
