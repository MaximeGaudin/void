/// Target number of characters per chunk (approximation of 384 tokens at ~4 chars/token).
const DEFAULT_CHUNK_CHARS: usize = 1536;

/// Overlap in characters between consecutive chunks (~64 tokens).
const DEFAULT_OVERLAP_CHARS: usize = 256;

/// Minimum chunk size in characters (~40 tokens). Smaller tails are merged.
const MIN_CHUNK_CHARS: usize = 160;

/// Maximum number of chunks per document to prevent pathological cases.
const MAX_CHUNKS_PER_DOC: usize = 2000;

#[derive(Debug, Clone)]
pub struct ChunkConfig {
    pub chunk_chars: usize,
    pub overlap_chars: usize,
    pub min_chunk_chars: usize,
    pub max_chunks: usize,
}

impl Default for ChunkConfig {
    fn default() -> Self {
        Self {
            chunk_chars: DEFAULT_CHUNK_CHARS,
            overlap_chars: DEFAULT_OVERLAP_CHARS,
            min_chunk_chars: MIN_CHUNK_CHARS,
            max_chunks: MAX_CHUNKS_PER_DOC,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextChunk {
    pub text: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub index: usize,
}

/// Split `text` into overlapping chunks respecting paragraph/sentence/word boundaries.
pub fn chunk_text(text: &str, config: &ChunkConfig) -> Vec<TextChunk> {
    if text.is_empty() {
        return vec![];
    }

    let paragraphs = split_paragraphs(text);
    let mut chunks: Vec<TextChunk> = Vec::new();
    let mut buf = String::new();
    let mut buf_start: usize = 0;
    let mut chunk_idx: usize = 0;

    for (para_text, para_start) in &paragraphs {
        if buf.len() + para_text.len() + 1 > config.chunk_chars && !buf.is_empty() {
            chunks.push(TextChunk {
                text: buf.clone(),
                start_byte: buf_start,
                end_byte: buf_start + buf.len(),
                index: chunk_idx,
            });
            chunk_idx += 1;
            if chunks.len() >= config.max_chunks {
                return chunks;
            }

            let overlap_text = extract_overlap(&buf, config.overlap_chars);
            let overlap_byte_offset = buf.len() - overlap_text.len();
            buf_start += overlap_byte_offset;
            buf = overlap_text;
        }

        if para_text.len() > config.chunk_chars {
            if !buf.is_empty() {
                chunks.push(TextChunk {
                    text: buf.clone(),
                    start_byte: buf_start,
                    end_byte: buf_start + buf.len(),
                    index: chunk_idx,
                });
                chunk_idx += 1;
                if chunks.len() >= config.max_chunks {
                    return chunks;
                }
                buf.clear();
                buf_start = *para_start;
            }

            let sub_chunks = split_long_paragraph(
                para_text,
                *para_start,
                config.chunk_chars,
                config.overlap_chars,
            );
            for sc in sub_chunks {
                chunks.push(TextChunk {
                    text: sc.0,
                    start_byte: sc.1,
                    end_byte: sc.2,
                    index: chunk_idx,
                });
                chunk_idx += 1;
                if chunks.len() >= config.max_chunks {
                    return chunks;
                }
            }
            buf.clear();
            if let Some(last) = chunks.last() {
                let overlap_text = extract_overlap(&last.text, config.overlap_chars);
                buf_start = last.end_byte - overlap_text.len();
                buf = overlap_text;
            }
        } else {
            if !buf.is_empty() {
                buf.push('\n');
            } else {
                buf_start = *para_start;
            }
            buf.push_str(para_text);
        }
    }

    if !buf.is_empty() {
        if buf.len() < config.min_chunk_chars && !chunks.is_empty() {
            let last = chunks.last_mut().unwrap();
            let gap = &text[last.end_byte..buf_start];
            last.text.push_str(gap);
            last.text.push_str(&buf);
            last.end_byte = buf_start + buf.len();
        } else {
            chunks.push(TextChunk {
                text: buf.clone(),
                start_byte: buf_start,
                end_byte: buf_start + buf.len(),
                index: chunk_idx,
            });
        }
    }

    chunks
}

fn split_paragraphs(text: &str) -> Vec<(String, usize)> {
    let mut result = Vec::new();
    let mut start = 0;

    for part in text.split("\n\n") {
        let trimmed = part.trim();
        if !trimmed.is_empty() {
            let offset = text[start..].find(trimmed).map(|i| start + i).unwrap_or(start);
            result.push((trimmed.to_string(), offset));
        }
        start += part.len() + 2; // +2 for the "\n\n"
    }

    result
}

fn split_long_paragraph(
    text: &str,
    base_offset: usize,
    target_size: usize,
    overlap: usize,
) -> Vec<(String, usize, usize)> {
    let sentences = split_sentences(text);
    let mut result = Vec::new();
    let mut buf = String::new();
    let mut buf_rel_start: usize = 0;

    for (sent, sent_start) in &sentences {
        if buf.len() + sent.len() + 1 > target_size && !buf.is_empty() {
            let abs_start = base_offset + buf_rel_start;
            result.push((buf.clone(), abs_start, abs_start + buf.len()));

            let overlap_text = extract_overlap(&buf, overlap);
            buf_rel_start += buf.len() - overlap_text.len();
            buf = overlap_text;
        }

        if sent.len() > target_size {
            if !buf.is_empty() {
                let abs_start = base_offset + buf_rel_start;
                result.push((buf.clone(), abs_start, abs_start + buf.len()));
                buf.clear();
                buf_rel_start = *sent_start;
            }
            let word_chunks = split_by_words(sent, *sent_start, base_offset, target_size, overlap);
            result.extend(word_chunks);
            if let Some((ref last_text, _, _)) = result.last() {
                let ov = extract_overlap(last_text, overlap);
                buf_rel_start = result.last().unwrap().2 - base_offset - ov.len();
                buf = ov;
            }
        } else {
            if !buf.is_empty() {
                buf.push(' ');
            } else {
                buf_rel_start = *sent_start;
            }
            buf.push_str(sent);
        }
    }

    if !buf.is_empty() {
        let abs_start = base_offset + buf_rel_start;
        result.push((buf.clone(), abs_start, abs_start + buf.len()));
    }

    result
}

fn split_sentences(text: &str) -> Vec<(String, usize)> {
    let mut result = Vec::new();
    let mut start = 0;
    let terminators = [". ", "? ", "! ", ".\n", "?\n", "!\n"];

    let mut pos = 0;
    while pos < text.len() {
        let mut found = false;
        for term in &terminators {
            if text[pos..].starts_with(term) {
                let end = pos + 1;
                let sentence = text[start..end].trim().to_string();
                if !sentence.is_empty() {
                    result.push((sentence, start));
                }
                start = end;
                while start < text.len()
                    && text.as_bytes().get(start).copied() == Some(b' ')
                {
                    start += 1;
                }
                pos = start;
                found = true;
                break;
            }
        }
        if !found {
            pos += 1;
        }
    }

    if start < text.len() {
        let tail = text[start..].trim().to_string();
        if !tail.is_empty() {
            result.push((tail, start));
        }
    }

    result
}

fn split_by_words(
    text: &str,
    sent_start: usize,
    base_offset: usize,
    target_size: usize,
    overlap: usize,
) -> Vec<(String, usize, usize)> {
    let mut result = Vec::new();
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut buf = String::new();
    let mut buf_rel_start: usize = sent_start;

    for word in &words {
        if buf.len() + word.len() + 1 > target_size && !buf.is_empty() {
            let abs_start = base_offset + buf_rel_start;
            result.push((buf.clone(), abs_start, abs_start + buf.len()));
            let ov = extract_overlap(&buf, overlap);
            buf_rel_start += buf.len() - ov.len();
            buf = ov;
        }
        if !buf.is_empty() {
            buf.push(' ');
        }
        buf.push_str(word);
    }
    if !buf.is_empty() {
        let abs_start = base_offset + buf_rel_start;
        result.push((buf.clone(), abs_start, abs_start + buf.len()));
    }
    result
}

fn extract_overlap(text: &str, max_chars: usize) -> String {
    if text.len() <= max_chars {
        return text.to_string();
    }
    let start = text.len() - max_chars;
    if let Some(ws) = text[start..].find(' ') {
        text[start + ws + 1..].to_string()
    } else {
        text[start..].to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> ChunkConfig {
        ChunkConfig::default()
    }

    #[test]
    fn empty_text() {
        let chunks = chunk_text("", &default_config());
        assert!(chunks.is_empty());
    }

    #[test]
    fn short_text_single_chunk() {
        let text = "Hello, world. This is a short document.";
        let chunks = chunk_text(text, &default_config());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, text);
        assert_eq!(chunks[0].start_byte, 0);
        assert_eq!(chunks[0].end_byte, text.len());
        assert_eq!(chunks[0].index, 0);
    }

    #[test]
    fn long_text_splits() {
        let paragraph = "word ".repeat(500);
        let text = format!("{paragraph}\n\n{paragraph}\n\n{paragraph}");
        let config = ChunkConfig {
            chunk_chars: 800,
            overlap_chars: 100,
            min_chunk_chars: 50,
            max_chunks: 100,
        };
        let chunks = chunk_text(&text, &config);
        assert!(chunks.len() > 1, "expected multiple chunks, got {}", chunks.len());
    }

    #[test]
    fn overlap_between_chunks() {
        let sentences: String = (0..100)
            .map(|i| format!("Sentence number {i} with some extra words to pad it. "))
            .collect();
        let config = ChunkConfig {
            chunk_chars: 200,
            overlap_chars: 50,
            min_chunk_chars: 30,
            max_chunks: 100,
        };
        let chunks = chunk_text(&sentences, &config);
        assert!(chunks.len() >= 2);

        for pair in chunks.windows(2) {
            let a_end = &pair[0].text;
            let b_start = &pair[1].text;
            let a_tail: String = a_end.chars().rev().take(30).collect::<String>().chars().rev().collect();
            assert!(
                b_start.contains(&a_tail) || pair[1].start_byte < pair[0].end_byte,
                "chunks should overlap"
            );
        }
    }

    #[test]
    fn paragraph_boundary_respected() {
        let p1 = "a ".repeat(300);
        let p2 = "b ".repeat(300);
        let text = format!("{}\n\n{}", p1.trim(), p2.trim());
        let config = ChunkConfig {
            chunk_chars: 700,
            overlap_chars: 50,
            min_chunk_chars: 30,
            max_chunks: 100,
        };
        let chunks = chunk_text(&text, &config);
        assert!(chunks.len() >= 2);
        assert!(chunks[0].text.starts_with("a "));
    }

    #[test]
    fn tiny_tail_merged() {
        let main = "word ".repeat(350);
        let tail = "end";
        let text = format!("{}\n\n{}", main.trim(), tail);
        let config = ChunkConfig {
            chunk_chars: 1800,
            overlap_chars: 100,
            min_chunk_chars: 160,
            max_chunks: 100,
        };
        let chunks = chunk_text(&text, &config);
        assert_eq!(chunks.len(), 1, "tiny tail should be merged");
        assert!(chunks[0].text.contains("end"));
    }

    #[test]
    fn unicode_multibyte() {
        let text = "こんにちは世界。これはテストです。";
        let chunks = chunk_text(text, &default_config());
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, text);
    }

    #[test]
    fn byte_offsets_valid() {
        let text = "Hello world.\n\nSecond paragraph here.";
        let chunks = chunk_text(text, &default_config());
        for chunk in &chunks {
            assert!(chunk.start_byte <= chunk.end_byte);
            assert!(chunk.end_byte <= text.len());
        }
    }

    #[test]
    fn max_chunks_enforced() {
        let text = "word ".repeat(100_000);
        let config = ChunkConfig {
            chunk_chars: 50,
            overlap_chars: 10,
            min_chunk_chars: 10,
            max_chunks: 5,
        };
        let chunks = chunk_text(&text, &config);
        assert!(chunks.len() <= 5);
    }

    #[test]
    fn indices_sequential() {
        let text = "a ".repeat(5000);
        let config = ChunkConfig {
            chunk_chars: 200,
            overlap_chars: 30,
            min_chunk_chars: 20,
            max_chunks: 100,
        };
        let chunks = chunk_text(&text, &config);
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(c.index, i);
        }
    }
}
