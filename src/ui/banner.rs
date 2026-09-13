use crate::app::query::WhoamiStatus;
use colored::Colorize;

pub fn render_whoami(status: &WhoamiStatus) {
    match status.active_orbit {
        Some(ref name) => {
            let email_str = status
                .orbit_email
                .as_deref()
                .or(status.live_email.as_deref())
                .unwrap_or("unknown");
            println!(
                "Active Orbit: {} ({})",
                name.bold().green(),
                email_str.cyan()
            );
        }
        None => {
            if let Some(ref email) = status.live_email {
                println!(
                    "Active Account: {} (not managed by any Orbit yet)",
                    email.cyan()
                );
            } else {
                println!("No active Google account found in Antigravity.");
            }
        }
    }
}

pub fn render_success(msg: &str) {
    println!("{} {}", "✓".green().bold(), msg);
}
