//! Message coalescing: split long messages into channel-safe chunks.

/// Configuration for message splitting.
pub struct ChunkConfig {
    /// Maximum characters per chunk (platform limit).
    pub max_chars: usize,
}

impl ChunkConfig {
    pub fn telegram() -> Self {
        Self { max_chars: 4000 }
    }
    pub fn discord() -> Self {
        Self { max_chars: 1900 }
    }
    pub fn slack() -> Self {
        Self { max_chars: 3900 }
    }
    pub fn default_limit() -> Self {
        Self { max_chars: 4000 }
    }
}

/// Split text into channel-safe chunks.
///
/// Prefers breaking at paragraph boundaries (`\n\n`), then newlines (`\n`),
/// then sentence endings (`. `), then spaces, and finally hard-cuts as a last resort.
pub fn split_message(text: &str, config: &ChunkConfig) -> Vec<String> {
    if text.is_empty() {
        return vec![];
    }
    if text.len() <= config.max_chars {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= config.max_chars {
            chunks.push(remaining.to_string());
            break;
        }

        let search = &remaining[..config.max_chars];
        let break_at = find_break_point(search);

        chunks.push(remaining[..break_at].trim_end().to_string());
        remaining = remaining[break_at..].trim_start_matches('\n');
        // Skip a single leading space after break (not in middle of word)
        if remaining.starts_with(' ') && !remaining.starts_with("  ") {
            remaining = &remaining[1..];
        }
    }

    chunks
}

/// Find the best break point within `text`, preferring natural boundaries.
fn find_break_point(text: &str) -> usize {
    // Try paragraph break
    if let Some(pos) = text.rfind("\n\n") {
        if pos > 0 {
            return pos;
        }
    }
    // Try newline
    if let Some(pos) = text.rfind('\n') {
        if pos > 0 {
            return pos;
        }
    }
    // Try sentence end
    if let Some(pos) = text.rfind(". ") {
        if pos > 0 {
            return pos + 1; // include the period
        }
    }
    // Try space
    if let Some(pos) = text.rfind(' ') {
        if pos > 0 {
            return pos;
        }
    }
    // Hard cut at char boundary
    let mut end = text.len();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_split_needed() {
        let config = ChunkConfig { max_chars: 100 };
        let chunks = split_message("Hello world", &config);
        assert_eq!(chunks, vec!["Hello world"]);
    }

    #[test]
    fn test_empty_input() {
        let config = ChunkConfig { max_chars: 100 };
        let chunks = split_message("", &config);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_paragraph_split() {
        let config = ChunkConfig { max_chars: 30 };
        let text = "First paragraph.\n\nSecond paragraph here.";
        let chunks = split_message(text, &config);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "First paragraph.");
        assert_eq!(chunks[1], "Second paragraph here.");
    }

    #[test]
    fn test_newline_split() {
        let config = ChunkConfig { max_chars: 20 };
        let text = "Line one here\nLine two here";
        let chunks = split_message(text, &config);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "Line one here");
        assert_eq!(chunks[1], "Line two here");
    }

    #[test]
    fn test_sentence_split() {
        let config = ChunkConfig { max_chars: 25 };
        let text = "First sentence. Second sentence here.";
        let chunks = split_message(text, &config);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "First sentence.");
        assert_eq!(chunks[1], "Second sentence here.");
    }

    #[test]
    fn test_hard_cut() {
        let config = ChunkConfig { max_chars: 10 };
        let text = "abcdefghijklmnopqrst";
        let chunks = split_message(text, &config);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0], "abcdefghij");
        assert_eq!(chunks[1], "klmnopqrst");
    }
}
