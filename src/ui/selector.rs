use inquire::{Select, error::InquireError};
use std::io::IsTerminal;

/// Installs a global panic hook to ensure terminal raw mode is restored and cursor is
/// shown even under `panic = "abort"`.
pub fn install_terminal_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::cursor::Show,
            crossterm::style::ResetColor
        );
        eprintln!();
        eprintln!("============================================================");
        eprintln!("💥 agy-orbit encountered an unexpected crash!");
        eprintln!(
            "Version: v{} | OS: {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        );
        eprintln!("Please report this issue at:");
        eprintln!("👉 https://github.com/gxwane/agy-orbit/issues/new");
        eprintln!("============================================================");
        eprintln!();
        default_hook(panic_info);
    }));
}

/// Strict dual-channel TTY detection.
/// Returns true ONLY if both stdin and stdout are interactive terminals.
pub fn is_interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

#[derive(Clone, Eq, PartialEq)]
pub struct OrbitChoice<'a> {
    pub name: &'a str,
    pub email: &'a str,
    pub is_active: bool,
}

impl<'a> std::fmt::Display for OrbitChoice<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.is_active {
            write!(f, "[active] {} ({})", self.name, self.email)
        } else {
            write!(f, "         {} ({})", self.name, self.email)
        }
    }
}

/// Prompt the user with an interactive TUI selector to switch Orbits.
/// Returns:
/// - `Ok(Some(name))` if user selected an Orbit
/// - `Ok(None)` if user cancelled (Esc / q)
/// - `Err(InquireError::OperationInterrupted)` if user pressed Ctrl+C
pub fn select_orbit_interactive<'a>(
    orbits: &'a [(&'a str, &'a str)],
    active_orbit: Option<&str>,
) -> Result<Option<&'a str>, InquireError> {
    if orbits.is_empty() {
        return Ok(None);
    }

    let items: Vec<OrbitChoice<'a>> = orbits
        .iter()
        .map(|(name, email)| OrbitChoice {
            name,
            email,
            is_active: active_orbit == Some(*name),
        })
        .collect();

    // Default cursor focus on the active orbit, if any
    let starting_cursor = active_orbit
        .and_then(|act| orbits.iter().position(|(name, _)| *name == act))
        .unwrap_or(0);

    let ans = Select::new("Select an Orbit to activate:", items)
        .with_starting_cursor(starting_cursor)
        .with_vim_mode(true)
        .with_help_message("↑/k: up • ↓/j: down • Enter: select • Esc/q: cancel")
        .prompt_skippable()?;

    Ok(ans.map(|choice| choice.name))
}
