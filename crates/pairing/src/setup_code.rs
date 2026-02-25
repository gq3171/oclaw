//! Setup code generation and validation.

use rand::Rng;

const CHARSET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
const CODE_LEN: usize = 8;

/// Generate a random setup code (8 chars, no ambiguous characters).
pub fn generate_setup_code() -> String {
    let mut rng = rand::thread_rng();
    (0..CODE_LEN)
        .map(|_| {
            let idx = rng.gen_range(0..CHARSET.len());
            CHARSET[idx] as char
        })
        .collect()
}

/// Validate format of a setup code.
pub fn validate_setup_code(code: &str) -> bool {
    code.len() == CODE_LEN && code.chars().all(|c| CHARSET.contains(&(c as u8)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_code_length() {
        let code = generate_setup_code();
        assert_eq!(code.len(), CODE_LEN);
    }

    #[test]
    fn test_generate_code_charset() {
        let code = generate_setup_code();
        assert!(validate_setup_code(&code));
    }

    #[test]
    fn test_validate_rejects_bad() {
        assert!(!validate_setup_code("short"));
        assert!(!validate_setup_code("abcdefgh")); // lowercase
        assert!(!validate_setup_code("ABCDEFG0")); // contains 0
        assert!(!validate_setup_code("ABCDEFGI")); // contains I
    }
}
