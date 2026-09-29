//! Small, dependency-free helpers shared across modules.
//!
//! Nothing here touches the network or logs caller data; it is pure, boundary-adjacent
//! arithmetic and string handling used to keep error bodies and size estimates bounded.

/// Truncates `s` to at most `max_bytes` bytes, backing off to the nearest earlier
/// UTF-8 character boundary so the result is always valid `str`.
///
/// This is used to bound the size of diagnostic text (for example an API error body)
/// without ever panicking on a multi-byte character that straddles the cut point.
pub(crate) fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    // `end == 0` is always a valid boundary, so this never panics.
    &s[..end]
}

/// Cheap, tokenizer-free token estimate: roughly four bytes per token.
///
/// This is a pre-flight guard, not a tokenizer. The server remains authoritative;
/// see [`crate::model::Model::context_tokens`] and `Client::with_context_check`.
pub(crate) fn estimate_tokens(byte_len: usize) -> usize {
    byte_len.div_ceil(4)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_not_truncate_when_within_limit() {
        assert_eq!(truncate_on_char_boundary("hello", 10), "hello");
        assert_eq!(truncate_on_char_boundary("hello", 5), "hello");
    }

    #[test]
    fn test_should_truncate_to_char_boundary() {
        // "é" is 2 bytes (0xC3 0xA9). Cutting at byte 1 would split it.
        let s = "aé";
        assert_eq!(truncate_on_char_boundary(s, 2), "a");
        assert_eq!(truncate_on_char_boundary(s, 1), "a");
    }

    #[test]
    fn test_should_truncate_multibyte_string_without_panicking() {
        let s = "日本語のテキスト".repeat(1000);
        let truncated = truncate_on_char_boundary(&s, 4096);
        assert!(truncated.len() <= 4096);
        assert!(s.starts_with(truncated));
    }

    #[test]
    fn test_should_estimate_tokens_from_byte_length() {
        assert_eq!(estimate_tokens(0), 0);
        assert_eq!(estimate_tokens(1), 1);
        assert_eq!(estimate_tokens(4), 1);
        assert_eq!(estimate_tokens(5), 2);
        assert_eq!(estimate_tokens(4000), 1000);
    }
}
