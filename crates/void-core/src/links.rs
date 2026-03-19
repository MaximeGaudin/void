/// Parsed components from a Slack message permalink.
///
/// URL format: `https://{workspace}.slack.com/archives/{channel_id}/p{ts_no_dot}`
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlackLink {
    /// Workspace subdomain, which typically matches the void account_id.
    pub workspace: String,
    /// Slack channel / conversation ID (e.g. `D09R63ASNEL`).
    pub channel_id: String,
    /// Slack message timestamp in dot notation (e.g. `1773903727.112369`).
    pub message_ts: String,
}

impl SlackLink {
    /// Try to parse a Slack permalink URL.
    ///
    /// Returns `None` if the input is not a recognised Slack link.
    pub fn parse(input: &str) -> Option<Self> {
        let url = input.trim();
        let path = url
            .strip_prefix("https://")
            .or_else(|| url.strip_prefix("http://"))?;

        let (host, rest) = path.split_once('/')?;
        let workspace = host.strip_suffix(".slack.com")?;
        if workspace.is_empty() {
            return None;
        }

        let rest = rest.strip_prefix("archives/")?;
        let (channel_id, ts_part) = rest.split_once('/')?;
        if channel_id.is_empty() {
            return None;
        }

        let ts_raw = ts_part.strip_prefix('p')?;
        // Strip query string / fragment if present
        let ts_raw = ts_raw.split(&['?', '#'][..]).next().unwrap_or(ts_raw);
        let message_ts = slack_ts_to_dot(ts_raw)?;

        Some(Self {
            workspace: workspace.to_string(),
            channel_id: channel_id.to_string(),
            message_ts,
        })
    }

    /// The void internal message ID: `{workspace}-{message_ts}`.
    pub fn to_message_id(&self) -> String {
        format!("{}-{}", self.workspace, self.message_ts)
    }

    /// The void conversation ID: `{workspace}-{channel_id}`.
    pub fn to_conversation_id(&self) -> String {
        format!("{}-{}", self.workspace, self.channel_id)
    }
}

/// Convert a Slack compact timestamp (no dot, `p` prefix already stripped)
/// into dotted notation.  The last 6 digits go after the dot.
///
/// `"1773903727112369"` → `"1773903727.112369"`
fn slack_ts_to_dot(raw: &str) -> Option<String> {
    if raw.len() <= 6 || !raw.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let dot_pos = raw.len() - 6;
    Some(format!("{}.{}", &raw[..dot_pos], &raw[dot_pos..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_standard_link() {
        let link = SlackLink::parse(
            "https://gladiaio.slack.com/archives/D09R63ASNEL/p1773903727112369",
        )
        .unwrap();
        assert_eq!(link.workspace, "gladiaio");
        assert_eq!(link.channel_id, "D09R63ASNEL");
        assert_eq!(link.message_ts, "1773903727.112369");
        assert_eq!(link.to_message_id(), "gladiaio-1773903727.112369");
        assert_eq!(link.to_conversation_id(), "gladiaio-D09R63ASNEL");
    }

    #[test]
    fn parse_link_with_query_string() {
        let link = SlackLink::parse(
            "https://foo.slack.com/archives/C123/p1234567890123456?thread_ts=1234567890.000000",
        )
        .unwrap();
        assert_eq!(link.message_ts, "1234567890.123456");
    }

    #[test]
    fn parse_rejects_non_slack_url() {
        assert!(SlackLink::parse("https://example.com/foo").is_none());
    }

    #[test]
    fn parse_rejects_malformed_timestamp() {
        assert!(SlackLink::parse(
            "https://x.slack.com/archives/C1/pshort"
        ).is_none());
    }

    #[test]
    fn parse_rejects_missing_channel() {
        assert!(SlackLink::parse(
            "https://x.slack.com/archives//p1234567890123456"
        ).is_none());
    }

    #[test]
    fn slack_ts_to_dot_works() {
        assert_eq!(
            slack_ts_to_dot("1773903727112369"),
            Some("1773903727.112369".to_string())
        );
    }

    #[test]
    fn slack_ts_to_dot_rejects_short() {
        assert_eq!(slack_ts_to_dot("12345"), None);
    }

    #[test]
    fn slack_ts_to_dot_rejects_non_numeric() {
        assert_eq!(slack_ts_to_dot("abc123def456ghi"), None);
    }
}
