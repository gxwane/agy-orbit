use colored::Colorize;
use inquire::{error::InquireError, Select};
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
        default_hook(panic_info);
    }));
}

/// Strict dual-channel TTY detection.
/// Returns true ONLY if both stdin and stdout are interactive terminals.
pub fn is_interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
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

    let items: Vec<String> = orbits
        .iter()
        .map(|(name, email)| {
            if active_orbit == Some(*name) {
                format!("● {} ({}) [active]", name.bold().green(), email.cyan())
            } else {
                format!("○ {} ({})", name, email)
            }
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

    match ans {
        Some(selected_str) => {
            let idx = orbits
                .iter()
                .position(|(name, _)| selected_str.contains(name))
                .unwrap_or(0);
            Ok(Some(orbits[idx].0))
        }
        None => Ok(None),
    }
}
