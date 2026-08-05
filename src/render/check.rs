use super::color::glyph;
use crate::model::CheckReport;
use colored::Colorize;

pub fn render(report: &CheckReport) {
    if report.issues.is_empty() {
        println!(
            "{} all clear -- nothing stale or dead",
            glyph(crate::model::Health::Healthy)
        );
        return;
    }
    for issue in &report.issues {
        println!(
            "{} {} {}",
            glyph(issue.health),
            issue.scope.bold(),
            issue.message
        );
    }
    println!();
    println!(
        "{}",
        format!("{} issue(s) found", report.issues.len())
            .red()
            .bold()
    );
}
