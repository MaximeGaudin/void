use unicode_normalization::UnicodeNormalization;

/// Normalize text for accent-insensitive, case-insensitive search.
///
/// Applies NFKD decomposition, strips combining marks (diacritics),
/// and folds to lowercase. Punctuation runs are collapsed to a single space.
pub fn normalize_for_search(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut last_was_space = true;

    for c in text.nfkd() {
        if is_combining_mark(c) {
            continue;
        }
        if c.is_alphanumeric() {
            for lc in c.to_lowercase() {
                result.push(lc);
            }
            last_was_space = false;
        } else if !last_was_space {
            result.push(' ');
            last_was_space = true;
        }
    }

    let trimmed = result.trim_end();
    trimmed.to_string()
}

fn is_combining_mark(c: char) -> bool {
    unicode_normalization::char::canonical_combining_class(c) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_passthrough() {
        assert_eq!(normalize_for_search("hello world"), "hello world");
    }

    #[test]
    fn accent_removal() {
        assert_eq!(normalize_for_search("café"), "cafe");
        assert_eq!(normalize_for_search("résumé"), "resume");
        assert_eq!(normalize_for_search("Zürich"), "zurich");
        assert_eq!(normalize_for_search("naïve"), "naive");
    }

    #[test]
    fn case_folding() {
        assert_eq!(normalize_for_search("HELLO"), "hello");
        assert_eq!(normalize_for_search("Hello World"), "hello world");
    }

    #[test]
    fn ligatures() {
        assert_eq!(normalize_for_search("\u{FB01}nance"), "finance");
    }

    #[test]
    fn cjk_passthrough() {
        assert_eq!(normalize_for_search("你好世界"), "你好世界");
    }

    #[test]
    fn emoji_passthrough() {
        let result = normalize_for_search("hello 🌍 world");
        assert!(result.contains("hello"));
        assert!(result.contains("world"));
    }

    #[test]
    fn empty_string() {
        assert_eq!(normalize_for_search(""), "");
    }

    #[test]
    fn whitespace_only() {
        assert_eq!(normalize_for_search("   "), "");
    }

    #[test]
    fn numbers_preserved() {
        assert_eq!(normalize_for_search("test123"), "test123");
    }

    #[test]
    fn punctuation_collapsed() {
        assert_eq!(normalize_for_search("hello, world!"), "hello world");
        assert_eq!(
            normalize_for_search("a...b---c"),
            "a b c"
        );
    }

    #[test]
    fn mixed_accents_and_case() {
        assert_eq!(
            normalize_for_search("CRÈME BRÛLÉE"),
            "creme brulee"
        );
    }

    #[test]
    fn no_trailing_space() {
        assert_eq!(normalize_for_search("hello! "), "hello");
    }
}
