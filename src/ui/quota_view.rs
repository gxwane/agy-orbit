use crate::app::quota::QuotaViewData;
use crate::domain::quota::QuotaBucket;
use chrono::{DateTime, Utc};
use colored::Colorize;

const BAR_WIDTH: usize = 20;

/// Render quota information in terminal with adaptive bars, countdown, and warning headers.
pub fn render_quota_view(data: &QuotaViewData) {
    println!();
    println!(
        "{} {} ({})",
        "Orbit Quota:".bold().cyan(),
        data.orbit_name.bold(),
        data.account_email.as_deref().unwrap_or("unknown").cyan()
    );

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
