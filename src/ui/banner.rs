use crate::app::query::WhoamiStatus;
use crate::domain::orbit::ActiveState;
use colored::Colorize;
use std::io::IsTerminal;

pub fn render_whoami(status: &WhoamiStatus) {
    match &status.active_state {
        ActiveState::Managed { name, email } => {
            println!("Active Orbit: {} ({})", name.bold().green(), email.cyan());
        }
        ActiveState::Unmanaged { email } => {
            println!(
                "Active Account: {} (not managed by any Orbit yet)",
                email.cyan()
            );
            if std::io::stdout().is_terminal() {
                println!(
                    "{}",
                    "💡 Tip: Run 'agyo save <name>' to save and manage this account in Orbit."
                        .dimmed()
                );
            }
        }
        ActiveState::Anonymous => {
            println!("No active Google account found in Antigravity.");
        }
    }
}

pub fn render_success(msg: &str) {
    println!("{} {}", "✓".green().bold(), msg);
}
