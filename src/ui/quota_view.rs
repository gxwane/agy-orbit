use crate::app::quota::{MultiQuotaRowData, QuotaViewData, RowStatus};
use crate::domain::quota::{MetricState, QuotaBucket};
use chrono::{DateTime, Utc};
use colored::Colorize;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{Attribute, Cell, Color, Row, Table};

const BAR_WIDTH: usize = 20;

fn deduplicate_model_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Vec<&'a str> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for name in names {
        if seen.insert(name) {
            out.push(name);
        }
    }
    out
}

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
        let now = summary.fetched_at.unwrap_or_else(Utc::now);

        let has_argon_group = summary.groups.iter().any(|g| g.is_argon_group());
        let has_gemini_group = summary.groups.iter().any(|g| g.is_gemini_standard_group());
        let has_claude_group = summary.groups.iter().any(|g| g.is_claude_group());

        // 1. Separate Argon buckets
        let (argon_buckets, non_argon_buckets): (Vec<&QuotaBucket>, Vec<&QuotaBucket>) =
            summary.buckets.iter().partition(|b| b.is_argon());

        // Render Gemini 4 Argon separate pool if not already rendered as a group
        if !has_argon_group && !argon_buckets.is_empty() {
            println!("{}", "GEMINI 4 ARGON".bold().underline());
            println!(
                "  {}",
                "For paid Pro plans, Argon quota is separate. Higher Argon quota (included as part of the larger Gemini quota bucket) is available on Ultra plans.".dimmed()
            );

            let model_names =
                deduplicate_model_names(argon_buckets.iter().map(|b| b.effective_name()));
            println!(
                "  {}",
                format!("Models: {}", model_names.join(", ")).dimmed()
            );

            let first = argon_buckets[0];
            let same_pool = argon_buckets.iter().all(|b| {
                (b.remaining_percentage() - first.remaining_percentage()).abs() < 0.1
                    && b.reset_time == first.reset_time
            });

            if same_pool {
                let mut rep = (*first).clone();
                rep.display_name = Some("Gemini 4 Argon Weekly Limit".into());
                rep.window = Some("weekly".into());
                render_bucket_row(&rep, "  ");
            } else {
                for bucket in &argon_buckets {
                    render_bucket_row(bucket, "  ");
                }
            }
            rendered_any = true;
            println!();
        }

        // 2. Separate standard Gemini models and other models
        let (gemini_all, mut other_buckets): (Vec<&QuotaBucket>, Vec<&QuotaBucket>) =
            non_argon_buckets
                .into_iter()
                .partition(|b| b.is_gemini_standard());

        if !has_gemini_group {
            // Split standard Gemini into 5-hour rolling pool vs weekly pool based on reset time
            let (gemini_5h, gemini_weekly): (Vec<&QuotaBucket>, Vec<&QuotaBucket>) = gemini_all
                .into_iter()
                .partition(|b| b.is_five_hour_relative(now));

            // Render Gemini 5-hour rolling pool
            if !gemini_5h.is_empty() {
                let first = gemini_5h[0];
                // For a single bucket, same_pool is trivially true.
                // For multiple buckets, all must share the same pool (within 0.1% pct delta and same reset).
                let same_pool = gemini_5h.len() == 1
                    || gemini_5h.iter().all(|b| {
                        (b.remaining_percentage() - first.remaining_percentage()).abs() < 0.1
                            && b.reset_time == first.reset_time
                    });

                if same_pool {
                    println!(
                        "{}",
                        "Gemini Flash & Pro Models (5-Hour Rolling Pool)"
                            .bold()
                            .underline()
                    );
                    let model_names =
                        deduplicate_model_names(gemini_5h.iter().map(|b| b.effective_name()));
                    println!(
                        "  {}",
                        format!("Models: {}", model_names.join(", ")).dimmed()
                    );

                    let mut rep = (*first).clone();
                    rep.display_name = Some("Five Hour Limit Remaining".into());
                    rep.window = Some("5h".into());
                    render_bucket_row(&rep, "  ");
                    rendered_any = true;
                    println!();
                } else {
                    other_buckets.extend(gemini_5h);
                }
            } else {
                other_buckets.extend(gemini_5h);
            }

            // Render Gemini Weekly pool
            if !gemini_weekly.is_empty() {
                let first = gemini_weekly[0];
                let same_pool = gemini_weekly.len() == 1
                    || gemini_weekly.iter().all(|b| {
                        (b.remaining_percentage() - first.remaining_percentage()).abs() < 0.1
                            && b.reset_time == first.reset_time
                    });

                if same_pool {
                    println!(
                        "{}",
                        "Gemini Tiered Models (Shared Weekly Pool)"
                            .bold()
                            .underline()
                    );
                    let model_names =
                        deduplicate_model_names(gemini_weekly.iter().map(|b| b.effective_name()));
                    println!(
                        "  {}",
                        format!("Models: {}", model_names.join(", ")).dimmed()
                    );

                    let mut rep = (*first).clone();
                    rep.display_name = Some("Weekly Limit Remaining".into());
                    rep.window = Some("weekly".into());
                    render_bucket_row(&rep, "  ");
                    rendered_any = true;
                    println!();
                } else {
                    other_buckets.extend(gemini_weekly);
                }
            } else {
                other_buckets.extend(gemini_weekly);
            }
        }

        // If no Claude group exists (e.g. consumer accounts), fold Claude models if present
        if !has_claude_group {
            let (claude_models, rem): (Vec<&QuotaBucket>, Vec<&QuotaBucket>) = other_buckets
                .into_iter()
                .partition(|b| b.is_claude_or_gpt());
            other_buckets = rem;

            if !claude_models.is_empty() {
                let first = claude_models[0];
                let same_pool = claude_models.len() == 1
                    || claude_models.iter().all(|b| {
                        (b.remaining_percentage() - first.remaining_percentage()).abs() < 0.1
                            && b.reset_time == first.reset_time
                    });

                if same_pool {
                    println!(
                        "{}",
                        "Claude and Partner Models (Shared Weekly Pool)"
                            .bold()
                            .underline()
                    );
                    let model_names =
                        deduplicate_model_names(claude_models.iter().map(|b| b.effective_name()));
                    println!(
                        "  {}",
                        format!("Models: {}", model_names.join(", ")).dimmed()
                    );

                    let mut rep = (*first).clone();
                    rep.display_name = Some("Weekly Limit Remaining".into());
                    rep.window = Some("weekly".into());
                    render_bucket_row(&rep, "  ");
                    rendered_any = true;
                    println!();
                } else {
                    other_buckets.extend(claude_models);
                }
            } else {
                other_buckets.extend(claude_models);
            }
        }

        // Only show internal/additional buckets if they have consumed quota (<99.9%)
        // or explicitly have display names / reset times, avoiding internal token clutter
        let significant_others: Vec<&&QuotaBucket> = other_buckets
            .iter()
            .filter(|b| {
                if has_gemini_group && b.is_gemini_standard() {
                    return false;
                }
                if has_claude_group && b.is_claude_or_gpt() {
                    return false;
                }
                if has_argon_group && b.is_argon() {
                    return false;
                }
                let id = b.bucket_id.to_lowercase();
                let is_internal_stub = (id.starts_with("chat_") || id.starts_with("tab_"))
                    && b.remaining_percentage() >= 99.9
                    && b.reset_time.is_none();
                !is_internal_stub
            })
            .collect();

        if !significant_others.is_empty() {
            println!("{}", "Additional Quota:".bold().underline());
            for bucket in significant_others {
                render_bucket_row(bucket, "  ");
                rendered_any = true;
            }
            println!();
        }
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

    let has_any_argon = rows.iter().any(|r| r.argon_wk_pct.is_some());

    let mut header_cells = vec![
        Cell::new("Orbit").fg(Color::Cyan),
        Cell::new("Account").fg(Color::Cyan),
        Cell::new("Gemini Wk").fg(Color::Cyan),
        Cell::new("Gemini 5h").fg(Color::Cyan),
    ];
    if has_any_argon {
        header_cells.push(Cell::new("Argon Wk").fg(Color::Cyan));
    }
    header_cells.extend(vec![
        Cell::new("Claude Wk").fg(Color::Cyan),
        Cell::new("Claude 5h").fg(Color::Cyan),
        Cell::new("Status").fg(Color::Cyan),
    ]);

    let mut table = Table::new();
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS)
        .set_header(header_cells);

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

        let gw_cell = format_metric_cell_with_reset(row.gemini_wk_pct, row.gemini_wk_reset);
        let g5_cell = format_metric_cell_with_reset(row.gemini_5h_pct, row.gemini_5h_reset);
        let cw_cell = format_metric_cell_with_reset(row.claude_wk_pct, row.claude_wk_reset);
        let c5_cell = format_metric_cell_with_reset(row.claude_5h_pct, row.claude_5h_reset);

        let (status_str, status_color) = match &row.status {
            RowStatus::Ready => ("Ready", Color::Green),
            RowStatus::Partial(label) => (label.as_str(), Color::Cyan),
            RowStatus::Throttled => ("Low Quota", Color::Yellow),
            RowStatus::Exhausted => ("Exhausted", Color::Red),
            RowStatus::TokenStale(_) => ("Token Stale", Color::Yellow),
            RowStatus::AuthExpired(_) => ("Auth Expired", Color::Red),
            RowStatus::Error(_) => ("Error", Color::Red),
        };
        let status_cell = Cell::new(status_str).fg(status_color);

        let mut row_cells = vec![orbit_cell, email_cell, gw_cell, g5_cell];
        if has_any_argon {
            row_cells.push(format_metric_cell_with_reset(
                row.argon_wk_pct,
                row.argon_wk_reset,
            ));
        }
        row_cells.extend(vec![cw_cell, c5_cell, status_cell]);

        table.add_row(Row::from(row_cells));
    }

    println!("{table}");
    println!(
        "{} {}",
        "*".green().bold(),
        "Active orbit  |  DIS: Disabled by Google policy (weekly quota exhausted)".dimmed()
    );

    let has_unmanaged = rows.iter().any(|r| r.orbit_name == "(unmanaged)");
    if has_unmanaged {
        println!(
            "{}",
            "Tip: Current active account is not managed by any Orbit. Run 'agyo save <name>' to save it."
                .yellow()
        );
    }

    // Display notices or warnings for non-ready states below table
    for row in rows {
        let target_type = if row.orbit_name == "(unmanaged)" {
            "Account"
        } else {
            "Orbit"
        };
        match &row.status {
            RowStatus::TokenStale(msg) => {
                println!(
                    "{} {} '{}': {}",
                    "ℹ Notice:".cyan().bold(),
                    target_type,
                    row.orbit_name.bold(),
                    msg.cyan()
                );
            }
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

fn format_metric_cell_with_reset(
    metric_opt: Option<MetricState>,
    reset_opt: Option<DateTime<Utc>>,
) -> Cell {
    match metric_opt {
        Some(MetricState::Disabled) => Cell::new("   DIS")
            .fg(Color::Red)
            .add_attribute(Attribute::Bold),
        Some(MetricState::Available(pct)) => {
            let s = if pct < 99.95
                && let Some(reset) = reset_opt
            {
                format!("{:>5.1}% ({})", pct, format_compact_countdown(reset))
            } else {
                format!("{:>5.1}%", pct)
            };
            if pct >= 50.0 {
                Cell::new(s).fg(Color::Green)
            } else if pct >= 20.0 {
                Cell::new(s).fg(Color::Yellow)
            } else {
                Cell::new(s).fg(Color::Red)
            }
        }
        None => Cell::new("     -").fg(Color::DarkGrey),
    }
}

#[cfg(test)]
fn format_metric_cell(metric_opt: Option<MetricState>) -> Cell {
    format_metric_cell_with_reset(metric_opt, None)
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

fn format_compact_countdown(reset_time: DateTime<Utc>) -> String {
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
        format!("{days}d {rem_hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m")
    } else {
        format!("{total_secs}s")
    }
}

fn format_countdown(reset_time: DateTime<Utc>) -> String {
    let compact = format_compact_countdown(reset_time);
    if compact == "now" {
        compact
    } else {
        format!("in {compact}")
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

    #[test]
    fn test_format_metric_cell() {
        let dis = format_metric_cell(Some(MetricState::Disabled));
        assert_eq!(dis.content(), "   DIS");

        let avail = format_metric_cell(Some(MetricState::Available(85.4)));
        assert_eq!(avail.content(), " 85.4%");

        let none = format_metric_cell(None);
        assert_eq!(none.content(), "     -");
    }

    #[test]
    fn test_deduplicate_model_names() {
        let input = vec![
            "Gemini 2.5 Flash",
            "Gemini 3.5 Flash Lite",
            "Gemini 3.5 Flash Lite",
            "Gemini 3.5 Flash Lite",
            "Gemini 3 Flash",
        ];
        let deduped = deduplicate_model_names(input);
        assert_eq!(
            deduped,
            vec![
                "Gemini 2.5 Flash",
                "Gemini 3.5 Flash Lite",
                "Gemini 3 Flash",
            ]
        );
    }

    #[test]
    fn test_render_multi_quota_table_argon_column_presence() {
        let rows_with_argon = vec![MultiQuotaRowData {
            orbit_name: "pro".into(),
            account_email: Some("pro@example.com".into()),
            is_active: true,
            is_stale: false,
            gemini_5h_pct: Some(MetricState::Available(100.0)),
            gemini_5h_reset: None,
            gemini_wk_pct: Some(MetricState::Available(100.0)),
            gemini_wk_reset: None,
            argon_wk_pct: Some(MetricState::Available(21.97)),
            argon_wk_reset: None,
            claude_5h_pct: None,
            claude_5h_reset: None,
            claude_wk_pct: None,
            claude_wk_reset: None,
            status: RowStatus::Ready,
            next_reset: None,
        }];
        // Should execute cleanly without panic
        render_multi_quota_table(&rows_with_argon);
    }
}
