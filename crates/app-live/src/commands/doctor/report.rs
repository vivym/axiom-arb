use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorOverallResult {
    Pass,
    Fail,
    PassWithSkips,
}

impl fmt::Display for DoctorOverallResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DoctorOverallResult::Pass => f.write_str("PASS"),
            DoctorOverallResult::Fail => f.write_str("FAIL"),
            DoctorOverallResult::PassWithSkips => f.write_str("PASS WITH SKIPS"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DoctorCheckStatus {
    Pass,
    Fail,
    Skip,
}

impl DoctorCheckStatus {
    fn marker(self) -> &'static str {
        match self {
            DoctorCheckStatus::Pass => "OK",
            DoctorCheckStatus::Fail => "FAIL",
            DoctorCheckStatus::Skip => "SKIP",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorCheckReport {
    pub status: DoctorCheckStatus,
    pub label: String,
    pub detail: String,
}

impl DoctorCheckReport {
    fn new(status: DoctorCheckStatus, label: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status,
            label: label.into(),
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoctorSectionReport {
    pub title: &'static str,
    pub checks: Vec<DoctorCheckReport>,
}

impl DoctorSectionReport {
    fn new(title: &'static str) -> Self {
        Self {
            title,
            checks: Vec::new(),
        }
    }

    fn push_check(
        &mut self,
        status: DoctorCheckStatus,
        label: impl Into<String>,
        detail: impl Into<String>,
    ) {
        self.checks
            .push(DoctorCheckReport::new(status, label, detail));
    }

    fn result(&self) -> DoctorOverallResult {
        if self
            .checks
            .iter()
            .any(|check| matches!(check.status, DoctorCheckStatus::Fail))
        {
            DoctorOverallResult::Fail
        } else if self
            .checks
            .iter()
            .any(|check| matches!(check.status, DoctorCheckStatus::Skip))
        {
            DoctorOverallResult::PassWithSkips
        } else {
            DoctorOverallResult::Pass
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct DoctorReport {
    sections: Vec<DoctorSectionReport>,
}

impl DoctorReport {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push_check(
        &mut self,
        section: &'static str,
        status: DoctorCheckStatus,
        label: impl Into<String>,
        detail: impl Into<String>,
    ) {
        self.section_mut(section).push_check(status, label, detail);
    }

    pub fn render(&self, next_actions: &[String]) {
        for section in &self.sections {
            for check in &section.checks {
                if check.detail.is_empty() {
                    println!("[{}] {}", check.status.marker(), check.label);
                } else {
                    println!(
                        "[{}] {}: {}",
                        check.status.marker(),
                        check.label,
                        check.detail
                    );
                }
            }
        }

        for section in &self.sections {
            println!("{}: {}", section.title, section.result());
        }
        println!("Overall: {}", self.overall_result());

        for action in next_actions {
            println!("Next: {action}");
        }
    }

    pub fn section_failed(&self, title: &'static str) -> bool {
        self.sections
            .iter()
            .find(|section| section.title == title)
            .map(|section| matches!(section.result(), DoctorOverallResult::Fail))
            .unwrap_or(false)
    }

    fn section_mut(&mut self, title: &'static str) -> &mut DoctorSectionReport {
        if let Some(index) = self
            .sections
            .iter()
            .position(|section| section.title == title)
        {
            return &mut self.sections[index];
        }

        self.sections.push(DoctorSectionReport::new(title));
        self.sections.last_mut().expect("section just pushed")
    }

    fn overall_result(&self) -> DoctorOverallResult {
        if self
            .sections
            .iter()
            .any(|section| matches!(section.result(), DoctorOverallResult::Fail))
        {
            DoctorOverallResult::Fail
        } else if self
            .sections
            .iter()
            .any(|section| matches!(section.result(), DoctorOverallResult::PassWithSkips))
        {
            DoctorOverallResult::PassWithSkips
        } else {
            DoctorOverallResult::Pass
        }
    }
}
