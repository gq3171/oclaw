/// Find the largest byte index <= `index` that is a valid UTF-8 char boundary.
/// Prevents panics when slicing multi-byte strings at arbitrary byte offsets.
pub fn floor_char_boundary(s: &str, index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    let mut i = index;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ascii() {
        assert_eq!(floor_char_boundary("hello", 3), 3);
        assert_eq!(floor_char_boundary("hello", 10), 5);
    }

    #[test]
    fn test_multibyte() {
        let s = "héllo"; // é is 2 bytes (0xC3 0xA9)
        // byte 0: 'h', byte 1-2: 'é', byte 3: 'l', byte 4: 'l', byte 5: 'o'
        assert_eq!(floor_char_boundary(s, 2), 1); // mid-é → back to 'h' end
        assert_eq!(floor_char_boundary(s, 1), 1); // start of 'é'
        assert_eq!(floor_char_boundary(s, 3), 3); // start of 'l'
    }

    #[test]
    fn test_cjk() {
        let s = "你好世界"; // each char is 3 bytes
        assert_eq!(floor_char_boundary(s, 4), 3); // mid-second char → end of first
        assert_eq!(floor_char_boundary(s, 6), 6); // exact boundary
    }

    #[test]
    fn test_empty() {
        assert_eq!(floor_char_boundary("", 0), 0);
        assert_eq!(floor_char_boundary("", 5), 0);
    }
}
