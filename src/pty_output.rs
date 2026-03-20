//! PTY output parsing utilities.
//!
//! Provides ANSI stripping, cost/context parsing for agent-driven PTY sessions.

use regex::Regex;

/// Strip ANSI escape codes from raw bytes and return a UTF-8 string.
pub fn strip_ansi(raw: &[u8]) -> String {
    let stripped = strip_ansi_escapes::strip(raw);
    String::from_utf8_lossy(&stripped).into_owned()
}

// ---------------------------------------------------------------------------
// Cost and context info structs
// ---------------------------------------------------------------------------

/// Token usage and cost information reported by an agent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CostInfo {
    pub total_cost_usd: Option<f64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub thinking_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

/// Context window usage reported by an agent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ContextInfo {
    pub used_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub used_percentage: Option<f64>,
}

// ---------------------------------------------------------------------------
// Parsed response
// ---------------------------------------------------------------------------

/// A parsed agent response, with both ANSI-stripped text and original bytes.
pub struct ParsedResponse {
    /// ANSI-stripped response text.
    pub text: String,
    /// Original raw bytes from the PTY.
    pub raw: Vec<u8>,
}

impl ParsedResponse {
    /// Build a `ParsedResponse` from raw PTY bytes.
    pub fn from_raw(raw: Vec<u8>) -> Self {
        let text = strip_ansi(&raw);
        Self { text, raw }
    }
}

// ---------------------------------------------------------------------------
// Cost / context parsing
// ---------------------------------------------------------------------------

/// Parse the output of Claude's `/cost` command into a [`CostInfo`].
///
/// Expected format (lines may appear in any order, all are optional):
/// ```text
/// Total cost: $0.054
/// Input tokens: 1245
/// Output tokens: 856
/// Thinking tokens: 0
/// Cache read tokens: 256
/// Cache write tokens: 512
/// ```
pub fn parse_claude_cost(output: &str) -> Option<CostInfo> {
    let cost_re = Regex::new(r"(?i)total\s+cost:\s*\$?([\d.]+)").ok()?;
    let input_re = Regex::new(r"(?i)input\s+tokens:\s*([\d,]+)").ok()?;
    let output_re = Regex::new(r"(?i)output\s+tokens:\s*([\d,]+)").ok()?;
    let thinking_re = Regex::new(r"(?i)thinking\s+tokens:\s*([\d,]+)").ok()?;
    let cache_read_re = Regex::new(r"(?i)cache\s+read\s+tokens:\s*([\d,]+)").ok()?;
    let cache_write_re = Regex::new(r"(?i)cache\s+write\s+tokens:\s*([\d,]+)").ok()?;

    let parse_u64 = |caps: regex::Captures| -> Option<u64> {
        caps.get(1)
            .map(|m| m.as_str().replace(',', ""))
            .and_then(|s| s.parse().ok())
    };

    let info = CostInfo {
        total_cost_usd: cost_re
            .captures(output)
            .and_then(|c| c.get(1)?.as_str().parse().ok()),
        input_tokens: input_re.captures(output).and_then(parse_u64),
        output_tokens: output_re.captures(output).and_then(parse_u64),
        thinking_tokens: thinking_re.captures(output).and_then(parse_u64),
        cache_read_tokens: cache_read_re.captures(output).and_then(parse_u64),
        cache_write_tokens: cache_write_re.captures(output).and_then(parse_u64),
    };

    // Return None only if every field is absent.
    if info.total_cost_usd.is_none()
        && info.input_tokens.is_none()
        && info.output_tokens.is_none()
        && info.thinking_tokens.is_none()
        && info.cache_read_tokens.is_none()
        && info.cache_write_tokens.is_none()
    {
        None
    } else {
        Some(info)
    }
}

/// Parse the output of Claude's `/context` command into a [`ContextInfo`].
///
/// Expected format:
/// ```text
/// Context: 45000/200000 tokens (22.5%)
/// ```
pub fn parse_claude_context(output: &str) -> Option<ContextInfo> {
    let re = Regex::new(
        r"(?i)context:\s*([\d,]+)\s*/\s*([\d,]+)\s*tokens\s*\(\s*([\d.]+)%\s*\)",
    )
    .ok()?;

    let caps = re.captures(output)?;
    let parse_u64 = |s: &str| -> Option<u64> { s.replace(',', "").parse().ok() };

    Some(ContextInfo {
        used_tokens: caps.get(1).and_then(|m| parse_u64(m.as_str())),
        total_tokens: caps.get(2).and_then(|m| parse_u64(m.as_str())),
        used_percentage: caps.get(3).and_then(|m| m.as_str().parse().ok()),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_basic() {
        let raw = b"\x1b[31mHello\x1b[0m";
        assert_eq!(strip_ansi(raw), "Hello");
    }

    #[test]
    fn test_strip_ansi_empty() {
        assert_eq!(strip_ansi(b""), "");
    }

    #[test]
    fn test_strip_ansi_no_escapes() {
        let raw = b"plain text";
        assert_eq!(strip_ansi(raw), "plain text");
    }

    #[test]
    fn test_parse_claude_cost_full() {
        let output = "\
Total cost: $0.054\n\
Input tokens: 1,245\n\
Output tokens: 856\n\
Thinking tokens: 0\n\
Cache read tokens: 256\n\
Cache write tokens: 512\n";

        let info = parse_claude_cost(output).expect("should parse");
        assert_eq!(info.total_cost_usd, Some(0.054));
        assert_eq!(info.input_tokens, Some(1245));
        assert_eq!(info.output_tokens, Some(856));
        assert_eq!(info.thinking_tokens, Some(0));
        assert_eq!(info.cache_read_tokens, Some(256));
        assert_eq!(info.cache_write_tokens, Some(512));
    }

    #[test]
    fn test_parse_claude_cost_partial() {
        let output = "Total cost: $0.010\nInput tokens: 500\n";
        let info = parse_claude_cost(output).expect("should parse");
        assert_eq!(info.total_cost_usd, Some(0.010));
        assert_eq!(info.input_tokens, Some(500));
        assert_eq!(info.output_tokens, None);
        assert_eq!(info.thinking_tokens, None);
        assert_eq!(info.cache_read_tokens, None);
        assert_eq!(info.cache_write_tokens, None);
    }

    #[test]
    fn test_parse_claude_cost_empty() {
        assert!(parse_claude_cost("nothing here").is_none());
    }

    #[test]
    fn test_parse_claude_context() {
        let output = "Context: 45000/200000 tokens (22.5%)";
        let info = parse_claude_context(output).expect("should parse");
        assert_eq!(info.used_tokens, Some(45000));
        assert_eq!(info.total_tokens, Some(200000));
        assert_eq!(info.used_percentage, Some(22.5));
    }

    #[test]
    fn test_parse_claude_context_with_commas() {
        let output = "Context: 45,000/200,000 tokens (22.5%)";
        let info = parse_claude_context(output).expect("should parse");
        assert_eq!(info.used_tokens, Some(45000));
        assert_eq!(info.total_tokens, Some(200000));
    }

    #[test]
    fn test_parsed_response_creation() {
        let raw = b"\x1b[32mHello world\x1b[0m".to_vec();
        let resp = ParsedResponse::from_raw(raw.clone());
        assert_eq!(resp.text, "Hello world");
        assert_eq!(resp.raw, raw);
    }
}
