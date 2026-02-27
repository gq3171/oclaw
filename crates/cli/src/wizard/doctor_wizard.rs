use crate::wizard::{info, success};
use oclaw_doctor_core::{CheckCategory, DiagnosticManager};

pub struct DoctorWizard;

impl DoctorWizard {
    pub fn run() {
        info("=== OCLAWS Doctor - Diagnostic Tool ===");
        println!();

        let manager = DiagnosticManager::new();

        println!("Running diagnostics...");
        println!();

        let issues = futures::executor::block_on(manager.run_all_checks());

        if issues.is_empty() {
            success("All checks passed! Your OCLAWS installation looks good.");
            return;
        }

        info(&format!("Found {} issue(s):", issues.len()));
        println!();

        for issue in &issues {
            println!("  - {}", issue.message);
        }
    }

    pub fn check_category(category: CheckCategory) {
        let manager = DiagnosticManager::new();

        let issues = futures::executor::block_on(manager.run_category_check(category));

        if issues.is_empty() {
            success(&format!("All {:?} checks passed!", category));
            return;
        }

        info(&format!(
            "Found {} issue(s) in {:?}:",
            issues.len(),
            category
        ));
        println!();

        for issue in &issues {
            println!("  - {}", issue.message);
        }
    }
}
