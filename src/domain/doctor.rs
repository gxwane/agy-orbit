use serde::{Deserialize, Serialize};

/// Status level of an individual diagnostic check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CheckStatus {
    Pass,
    Info,
    Warn,
    Fail,
}

impl CheckStatus {
    pub fn symbol(&self) -> &'static str {
        match self {
            CheckStatus::Pass => "✔",
            CheckStatus::Info => "ℹ",
            CheckStatus::Warn => "⚠",
            CheckStatus::Fail => "✖",
        }
    }

    pub fn ascii_tag(&self) -> &'static str {
        match self {
            CheckStatus::Pass => "[PASS]",
            CheckStatus::Info => "[INFO]",
            CheckStatus::Warn => "[WARN]",
            CheckStatus::Fail => "[FAIL]",
        }
    }
}

/// An individual diagnostic item with status, summary, and optional actionable guidance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticItem {
    pub name: String,
    pub status: CheckStatus,
    pub summary: String,
    pub details: Option<String>,
    pub recommendation: Option<String>,
}

impl DiagnosticItem {
    pub fn new(name: impl Into<String>, status: CheckStatus, summary: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status,
            summary: summary.into(),
            details: None,
            recommendation: None,
        }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }

    pub fn with_recommendation(mut self, rec: impl Into<String>) -> Self {
        self.recommendation = Some(rec.into());
        self
    }
}

/// A grouped section of related diagnostic checks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiagnosticSection {
    pub title: String,
    pub items: Vec<DiagnosticItem>,
}

impl DiagnosticSection {
    pub fn new(title: impl Into<String>, items: Vec<DiagnosticItem>) -> Self {
        Self {
            title: title.into(),
            items,
        }
    }
}

/// Complete diagnostic report aggregated across all 5 dimensions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DoctorReport {
    pub sections: Vec<DiagnosticSection>,
    pub overall_healthy: bool,
    pub issue_count: usize,
    pub warn_count: usize,
    pub pass_count: usize,
    pub info_count: usize,
}

impl DoctorReport {
    pub fn new(sections: Vec<DiagnosticSection>) -> Self {
        let mut pass_count = 0;
        let mut info_count = 0;
        let mut warn_count = 0;
        let mut issue_count = 0;

        for section in &sections {
            for item in &section.items {
                match item.status {
                    CheckStatus::Pass => pass_count += 1,
                    CheckStatus::Info => info_count += 1,
                    CheckStatus::Warn => warn_count += 1,
                    CheckStatus::Fail => issue_count += 1,
                }
            }
        }

        Self {
            sections,
            overall_healthy: issue_count == 0,
            issue_count,
            warn_count,
            pass_count,
            info_count,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_doctor_report_aggregation() {
        let section = DiagnosticSection::new(
            "System",
            vec![
                DiagnosticItem::new("OS", CheckStatus::Pass, "Windows 11"),
                DiagnosticItem::new("CLI", CheckStatus::Warn, "Gemini CLI prefix")
                    .with_recommendation("Consider Antigravity CLI"),
                DiagnosticItem::new("Keyring", CheckStatus::Fail, "Missing keyring")
                    .with_recommendation("Run agy"),
                DiagnosticItem::new("Proxy", CheckStatus::Info, "Direct"),
            ],
        );

        let report = DoctorReport::new(vec![section]);
        assert_eq!(report.pass_count, 1);
        assert_eq!(report.warn_count, 1);
        assert_eq!(report.issue_count, 1);
        assert_eq!(report.info_count, 1);
        assert!(!report.overall_healthy);
    }
}
