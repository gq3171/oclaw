use crate::wizard::{info, prompt, prompt_yes_no};
use oclaws_config::Config;

pub struct SkillWizard;

impl SkillWizard {
    pub fn run() -> Result<Config, String> {
        info("=== Skill Setup Wizard ===");
        println!();

        let mut config = Config::default();

        info("Available built-in skills:");
        println!();
        println!("  1. Fetch      - Fetch web pages");
        println!("  2. Browser    - Browser automation");
        println!("  3. Git        - Git operations");
        println!("  4. Bash       - Shell commands");
        println!();

        let selection = prompt("Select skills to enable (comma-separated numbers, or 'all')");

        if selection == "all" || selection.is_empty() {
            info("All skills will be enabled by default.");
        } else {
            info(&format!("Enabled skills: {}", selection));
        }

        Ok(config)
    }
}
