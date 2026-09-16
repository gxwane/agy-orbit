use crate::domain::doctor::{CheckStatus, DoctorReport};
use colored::Colorize;
use std::io::IsTerminal;

/// Render the complete DoctorReport to stdout with TTY adaptation and actionable guidance.
pub fn render_doctor_report(report: &DoctorReport) {
    let is_tty = std::io::stdout().is_terminal();

    println!();
    if is_tty {
        println!("{}", "agy-orbit Diagnostic Doctor".bold().underline());
    } else {
        println!("agy-orbit Diagnostic Doctor");
    }
    println!();

    let total_sections = report.sections.len();
    for (idx, section) in report.sections.iter().enumerate() {
        let section_num = idx + 1;
        if is_tty {
            println!(
                "{} [{}/{}] {} {}",
                "───".dimmed(),
                section_num,
                total_sections,
                section.title.bold(),
                "────────────────────────────────────────".dimmed()
            );
        } else {
            println!(
                "--- [{}/{}] {} ---",
                section_num, total_sections, section.title
            );
        }

        for item in &section.items {
            let symbol_str = if is_tty {
                match item.status {
                    CheckStatus::Pass => "✔".green().bold().to_string(),
                    CheckStatus::Info => "ℹ".cyan().bold().to_string(),
                    CheckStatus::Warn => "⚠".yellow().bold().to_string(),
                    CheckStatus::Fail => "✖".red().bold().to_string(),
                }
            } else {
                item.status.ascii_tag().to_string()
            };

            let name_str = if is_tty {
                item.name.bold().to_string()
            } else {
                item.name.clone()
            };

            println!("  {symbol_str} {name_str}: {}", item.summary);

            if let Some(ref details) = item.details {
                if is_tty {
                    println!("    {}", details.dimmed());
                } else {
                    println!("    Details: {details}");
                }
            }

            if let Some(ref rec) = item.recommendation {
                if is_tty {
                    println!("    {} {}", "↳ Hint:".yellow().italic(), rec);
                } else {
                    println!("    -> Hint: {rec}");
                }
            }
        }
        println!();
    }

    // Diagnostic Summary Card
    let divider = "═".repeat(60);
    if is_tty {
        println!("{}", divider.dimmed());
    } else {
        println!("{divider}");
    }

    let summary_line = format!(
        "Diagnostic Summary: {} passed, {} info, {} warning(s), {} failure(s)",
        report.pass_count, report.info_count, report.warn_count, report.issue_count
    );

    let status_str = if report.issue_count > 0 {
        if is_tty {
            "ISSUES DETECTED".red().bold().to_string()
        } else {
            "ISSUES DETECTED".to_string()
        }
    } else if report.warn_count > 0 {
        if is_tty {
            "ATTENTION REQUIRED".yellow().bold().to_string()
        } else {
            "ATTENTION REQUIRED".to_string()
        }
    } else if is_tty {
        "HEALTHY".green().bold().to_string()
    } else {
        "HEALTHY".to_string()
    };

    println!("  {summary_line}");
    println!("  Overall Status: {status_str}");

    if is_tty {
        println!("{}", divider.dimmed());
    } else {
        println!("{divider}");
    }

    // Actionable Next Steps (if any Warn or Fail items present)
    let actionable_items: Vec<_> = report
        .sections
        .iter()
        .flat_map(|s| &s.items)
        .filter(|item| {
            (item.status == CheckStatus::Warn || item.status == CheckStatus::Fail)
                && item.recommendation.is_some()
        })
        .collect();

    if !actionable_items.is_empty() {
        println!();
        if is_tty {
            println!("{}", "Actionable Next Steps:".bold().yellow());
        } else {
            println!("Actionable Next Steps:");
        }

        for (idx, item) in actionable_items.iter().enumerate() {
            let num = idx + 1;
            let tag = if is_tty {
                match item.status {
                    CheckStatus::Warn => "[WARN]".yellow().to_string(),
                    CheckStatus::Fail => "[FAIL]".red().bold().to_string(),
                    _ => "".to_string(),
                }
            } else {
                item.status.ascii_tag().to_string()
            };

            let rec = item.recommendation.as_deref().unwrap_or("");
            println!("  {num}. {tag} {}: {rec}", item.name);
        }
    } else if report.overall_healthy && report.warn_count == 0 {
        println!();
        if is_tty {
            println!(
                "  {}",
                "All checks passed! Your Antigravity and Orbit environment is operating normally."
                    .green()
            );
        } else {
            println!(
                "  All checks passed! Your Antigravity and Orbit environment is operating normally."
            );
        }
    }
    println!();
}
