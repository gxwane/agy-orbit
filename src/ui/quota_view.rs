use crate::app::quota::{MultiQuotaRowData, QuotaViewData, RowStatus};
use crate::domain::quota::QuotaBucket;
use chrono::{DateTime, Utc};
use colored::Colorize;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Cell, Color, Row, Table};

const BAR_WIDTH: usize = 20;

/// Render quota information in terminal with adaptive bars, countdown, and warning headers.
pub fn render_quota_view(data: &QuotaViewData) {
    println!();
    if data.orbit_name == "(unmanaged)" {
        println!(
            "{} {}",
            "Active Account Quota:".bold().cyan(),
            data.account_email.as_deref().unwrap_or("unknown").cyan()
        );
    } else {
        println!(
            "{} {} ({})",
            "Orbit Quota:".bold().cyan(),
            data.orbit_name.bold(),
            data.account_email.as_deref().unwrap_or("unknown").cyan()
        );
    }

    if let Some(ref warn) = data.warning {
        println!("{} {}", "⚠ Warning:".yellow().bold(), warn.yellow());
    } else if data.is_stale {
        println!("{}", "ℹ Showing cached quota data.".yellow());
    }
    println!();

    let summary = &data.summary;
    let mut rendered_any = false;

    // Render grouped buckets if groups exist
    if !summary.groups.is_empty() {
        for group in &summary.groups {
            let group_name = sanitize_str(group.display_name.as_deref().unwrap_or("Models"));
            println!("{}", group_name.bold().underline());
            if let Some(ref desc) = group.description {
                let clean_desc = sanitize_str(desc);
                if !clean_desc.is_empty() {
                    println!("  {}", clean_desc.dimmed());
                }
            }

            for bucket in &group.buckets {
                render_bucket_row(bucket, "  ");
                rendered_any = true;
            }
            println!();
        }
    }

    // Render top-level buckets if present and not already covered by groups
    if !summary.buckets.is_empty() {
        if !summary.groups.is_empty() {
            println!("{}", "Additional Quota:".bold().underline());
        }
        for bucket in &summary.buckets {
            render_bucket_row(bucket, "  ");
            rendered_any = true;
        }
        println!();
    }

    if !rendered_any {
        println!(
            "{}",
            "  No model quota buckets returned by Google Cloud Code PA.".dimmed()
        );
        println!();
    }

    if let Some(fetched) = summary.fetched_at {
        let age = Utc::now() - fetched;
        let age_str = if age.num_seconds() < 60 {
            "just now".to_string()
        } else {
            format!("{}m ago", age.num_minutes())
        };
        println!("  {}", format!("Last updated: {age_str}").dimmed());
    }
    println!();
}

/// Render a subtle ergonomics tip if the user has multiple orbits configured.
pub fn render_quota_tip_if_multiple(total_orbits: usize) {
    if total_orbits > 1 {
        println!(
            "{}",
            "Tip: Run 'agyo quota -a' to inspect all accounts at a glance.".dimmed()
        );
        println!();
    }
}

/// Render aggregated multi-account quota dashboard table.
pub fn render_multi_quota_table(rows: &[MultiQuotaRowData]) {
    if rows.is_empty() {
        println!("No orbits found to display quota.");
        return;
    }

    println!();
    println!(
        "{}",
        "Antigravity Multi-Account Quota Dashboard".bold().cyan()
    );
    println!();

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(vec![
            Cell::new("Orbit").fg(Color::Cyan),
            Cell::new("Account").fg(Color::Cyan),
            Cell::new("Gemini 5h").fg(Color::Cyan),
            Cell::new("Gemini Wk").fg(Color::Cyan),
            Cell::new("Claude 5h").fg(Color::Cyan),
            Cell::new("Claude Wk").fg(Color::Cyan),
            Cell::new("Status").fg(Color::Cyan),
            Cell::new("Next Reset").fg(Color::Cyan),
        ]);

    for row in rows {
        let orbit_name = if row.is_active {
            format!("* {}", row.orbit_name)
        } else {
            format!("  {}", row.orbit_name)
        };

        let orbit_cell = if row.is_active {
            Cell::new(orbit_name).fg(Color::Green)
        } else {
            Cell::new(orbit_name)
        };

        let email_str = row.account_email.as_deref().unwrap_or("unknown");
        let email_cell = Cell::new(email_str);

        let g5_cell = format_pct_cell(row.gemini_5h_pct);
        let gw_cell = format_pct_cell(row.gemini_wk_pct);
        let c5_cell = format_pct_cell(row.claude_5h_pct);
        let cw_cell = format_pct_cell(row.claude_wk_pct);

        let (status_str, status_color) = match &row.status {
            RowStatus::Active => ("Active", Color::Green),
            RowStatus::Fresh => ("Fresh", Color::Green),
            RowStatus::Cached => ("Cached", Color::Yellow),
            RowStatus::Refreshed => ("Refreshed", Color::Cyan),
            RowStatus::AuthExpired(_) => ("Auth Expired", Color::Red),
            RowStatus::Error(_) => ("Error", Color::Red),
        };
        let status_cell = Cell::new(status_str).fg(status_color);

        let reset_str = match row.next_reset {
            Some(reset) => format_countdown(reset),
            None => "-".to_string(),
        };
        let reset_cell = Cell::new(reset_str);

        table.add_row(Row::from(vec![
            orbit_cell,
            email_cell,
            g5_cell,
            gw_cell,
            c5_cell,
            cw_cell,
            status_cell,
            reset_cell,
        ]));
    }

    println!("{table}");
    println!(
        "{} {}",
        "*".green().bold(),
        "Indicates currently active account or Orbit".dimmed()
    );

    let has_unmanaged = rows.iter().any(|r| r.orbit_name == "(unmanaged)");
    if has_unmanaged {
        println!(
            "{}",
            "Tip: Current active account is not managed by any Orbit. Run 'agyo save <name>' to save it."
                .yellow()
        );
    }

    // If any row has AuthExpired or Error, show detail warnings below table
    for row in rows {
        let target_type = if row.orbit_name == "(unmanaged)" {
            "Account"
        } else {
            "Orbit"
        };
        match &row.status {
            RowStatus::AuthExpired(msg) => {
                println!(
                    "{} {} '{}': {}",
                    "⚠ Warning:".yellow().bold(),
                    target_type,
                    row.orbit_name.bold(),
                    msg.yellow()
                );
            }
            RowStatus::Error(msg) => {
                println!(
                    "{} {} '{}': {}",
                    "⚠ Warning:".yellow().bold(),
                    target_type,
                    row.orbit_name.bold(),
                    msg.yellow()
                );
            }
            _ => {}
        }
    }
    println!();
}

fn format_pct_cell(pct_opt: Option<f64>) -> Cell {
    match pct_opt {
        Some(pct) => {
            let s = format!("{:>5.1}%", pct);
            if pct >= 50.0 {
                Cell::new(s).fg(Color::Green)
            } else if pct >= 20.0 {
                Cell::new(s).fg(Color::Yellow)
            } else {
                Cell::new(s).fg(Color::Red)
            }
        }
        None => Cell::new("    -").fg(Color::DarkGrey),
    }
}

fn render_bucket_row(bucket: &QuotaBucket, indent: &str) {
    let name = sanitize_str(bucket.effective_name());
    let pct = bucket.remaining_percentage();
    let bar = build_progress_bar(pct, bucket.disabled == Some(true));

    let window_info = match bucket.window.as_deref() {
        Some(w) if !w.trim().is_empty() => format!(" [{w}]"),
        _ => String::new(),
    };

    let reset_info = match bucket.reset_time {
        Some(reset) => format!(" (resets {})", format_countdown(reset)),
        None => String::new(),
    };

    println!(
        "{}{:<32} {}{}{}",
        indent,
        name.bold(),
        bar,
        window_info.dimmed(),
        reset_info.cyan()
    );
}

fn build_progress_bar(percentage: f64, is_disabled: bool) -> String {
    if is_disabled {
        let empty = "░".repeat(BAR_WIDTH);
        return format!("[{}] {}", empty.dimmed(), "DISABLED".red().bold());
    }

    let filled_slots = ((percentage / 100.0) * (BAR_WIDTH as f64)).round() as usize;
    let filled_slots = filled_slots.min(BAR_WIDTH);
    let empty_slots = BAR_WIDTH - filled_slots;

    let filled_str = "█".repeat(filled_slots);
    let empty_str = "░".repeat(empty_slots);

    let colored_bar = if percentage >= 50.0 {
        format!("{}{}", filled_str.green(), empty_str.dimmed())
    } else if percentage >= 20.0 {
        format!("{}{}", filled_str.yellow(), empty_str.dimmed())
    } else {
        format!("{}{}", filled_str.red(), empty_str.dimmed())
    };

    format!("[{}] {:>5.1}%", colored_bar, percentage)
}

fn format_countdown(reset_time: DateTime<Utc>) -> String {
    let now = Utc::now();
    let diff = reset_time - now;
    let total_secs = diff.num_seconds();

    if total_secs <= 0 {
        return "now".to_string();
    }

    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;

    if hours >= 24 {
        let days = hours / 24;
        let rem_hours = hours % 24;
        format!("in {days}d {rem_hours}h")
    } else if hours > 0 {
        format!("in {hours}h {minutes}m")
    } else if minutes > 0 {
        format!("in {minutes}m")
    } else {
        format!("in {total_secs}s")
    }
}

/// Sanitize text by stripping control characters and ANSI escape sequences.
fn sanitize_str(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_control() || *c == ' ' || *c == '\t')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_progress_bar_levels() {
        let high = build_progress_bar(80.0, false);
        assert!(high.contains("80.0%"));

        let mid = build_progress_bar(30.0, false);
        assert!(mid.contains("30.0%"));

        let low = build_progress_bar(10.0, false);
        assert!(low.contains("10.0%"));

        let disabled = build_progress_bar(0.0, true);
        assert!(disabled.contains("DISABLED"));
    }

    #[test]
    fn test_format_countdown() {
        let future_2h = Utc::now() + chrono::Duration::hours(2) + chrono::Duration::minutes(15);
        let s = format_countdown(future_2h);
        assert!(s.contains("2h 15m") || s.contains("2h 14m"));

        let past = Utc::now() - chrono::Duration::seconds(10);
        assert_eq!(format_countdown(past), "now");
    }

    #[test]
    fn test_sanitize_str() {
        let malicious = "Gemini\x1b[31m Pro\r\n";
        assert_eq!(sanitize_str(malicious), "Gemini[31m Pro");
    }
}
