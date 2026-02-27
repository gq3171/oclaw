use std::io::{self, Write};
use std::path::PathBuf;

pub mod channel_wizard;
pub mod config_wizard;
pub mod doctor_wizard;
pub mod provider_setup;
pub mod skill_wizard;

pub use channel_wizard::ChannelWizard;
pub use config_wizard::ConfigWizard;
pub use doctor_wizard::DoctorWizard;
pub use provider_setup::ProviderSetup;
pub use skill_wizard::SkillWizard;

pub fn welcome() {
    println!();
    println!("╔═══════════════════════════════════════════════════════════════╗");
    println!("║                     OCLAWS Wizard                          ║");
    println!("║              Open CLAW System - Setup                      ║");
    println!("╚═══════════════════════════════════════════════════════════════╝");
    println!();
}

pub fn prompt(prompt: &str) -> String {
    print!("{}: ", prompt);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

pub fn prompt_optional(msg: &str, current: Option<&str>) -> Option<String> {
    let display = current.unwrap_or("<not set>");
    print!("{} [{}]: ", msg, display);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let input = input.trim();
    if input.is_empty() {
        current.map(|s| s.to_string())
    } else {
        Some(input.to_string())
    }
}

#[allow(dead_code)]
pub fn prompt_password(prompt: &str) -> String {
    print!("{}: ", prompt);
    io::stdout().flush().unwrap();
    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    input.trim().to_string()
}

pub fn prompt_yes_no(prompt: &str, default: bool) -> bool {
    let default_str = if default { "[Y/n]" } else { "[y/N]" };
    loop {
        print!("{} {} ", prompt, default_str);
        io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim().to_lowercase();

        if input.is_empty() {
            return default;
        }

        match input.as_str() {
            "y" | "yes" => return true,
            "n" | "no" => return false,
            _ => println!("Please enter 'y' or 'n'"),
        }
    }
}

#[allow(dead_code)]
pub fn select_option<T: AsRef<str>>(prompt: &str, options: &[T], default: usize) -> usize {
    println!();
    println!("{}", prompt);
    for (i, opt) in options.iter().enumerate() {
        if i == default {
            println!("  {}. {} (default)", i + 1, opt.as_ref());
        } else {
            println!("  {}. {}", i + 1, opt.as_ref());
        }
    }
    println!();

    loop {
        print!("Select option: ");
        io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        let input = input.trim();

        if input.is_empty() {
            return default;
        }

        if let Ok(selection) = input.parse::<usize>()
            && selection >= 1
            && selection <= options.len()
        {
            return selection - 1;
        }
        println!("Invalid selection. Please try again.");
    }
}

pub fn get_config_dir() -> PathBuf {
    let config_dir = dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("oclaw");

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).ok();
    }

    config_dir
}

#[allow(dead_code)]
pub fn get_data_dir() -> PathBuf {
    let data_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("oclaw");

    if !data_dir.exists() {
        std::fs::create_dir_all(&data_dir).ok();
    }

    data_dir
}

pub fn success(message: &str) {
    println!("✓ {}", message);
}

pub fn error(message: &str) {
    eprintln!("✗ {}", message);
}

pub fn info(message: &str) {
    println!("ℹ {}", message);
}
