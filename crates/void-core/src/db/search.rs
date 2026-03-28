//! FTS5 search and query escaping for messages.

use tracing::debug;

use super::row;
use super::Database;
use crate::models::Message;

/// Escape a user query for FTS5 MATCH by quoting each term.
/// Characters like `@`, `-`, `*` are FTS5 operators and cause syntax errors
/// if passed raw.
pub fn fts5_escape(query: &str) -> String {
    let terms: Vec<&str> = query.split_whitespace().collect();
    if terms.is_empty() {
        return "\"\"".to_string();
    }
    terms
        .iter()
        .map(|t| {
            let escaped = t.replace('"', "\"\"");
            format!("\"{escaped}\"")
        })
        .collect::<Vec<_>>()
        .join(" ")
}

impl Database {
    pub fn search_messages(
        &self,
        query: &str,
        connection_filter: Option<&str>,
        connector_filter: Option<&str>,
        limit: i64,
        include_muted: bool,
    ) -> Result<Vec<Message>, crate::error::DbError> {
        let (results, _) = self.search_messages_paginated(
            query,
            connection_filter,
            connector_filter,
            limit,
            0,
            include_muted,
        )?;
        Ok(results)
    }

    pub fn search_messages_paginated(
        &self,
        query: &str,
        connection_filter: Option<&str>,
        connector_filter: Option<&str>,
        limit: i64,
        offset: i64,
        include_muted: bool,
    ) -> Result<(Vec<Message>, i64), crate::error::DbError> {
        let escaped = fts5_escape(query);
        let mut sql = String::from(
            "SELECT m.id, m.conversation_id, m.connection_id, m.connector, m.external_id, m.sender, m.sender_name, m.body, m.timestamp, m.synced_at, m.is_archived, m.reply_to_id, m.media_type, m.metadata, m.context_id
             FROM messages_fts fts
             JOIN messages m ON m.rowid = fts.rowid
             WHERE messages_fts MATCH ?1",
        );
        let mut count_sql = String::from(
            "SELECT COUNT(*)
             FROM messages_fts fts
             JOIN messages m ON m.rowid = fts.rowid
             WHERE messages_fts MATCH ?1",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = vec![Box::new(escaped)];

        if !include_muted {
            let muted_clause = " AND NOT EXISTS (SELECT 1 FROM conversations c WHERE c.id = m.conversation_id AND c.is_muted = 1)";
            sql.push_str(muted_clause);
            count_sql.push_str(muted_clause);
        }
        if let Some(acct) = connection_filter {
            let pattern = format!("%{acct}%");
            let clause = format!(" AND m.connection_id LIKE ?{}", param_values.len() + 1);
            sql.push_str(&clause);
            count_sql.push_str(&clause);
            param_values.push(Box::new(pattern));
        }
        if let Some(conn_type) = connector_filter {
            let clause = format!(" AND m.connector = ?{}", param_values.len() + 1);
            sql.push_str(&clause);
            count_sql.push_str(&clause);
            param_values.push(Box::new(conn_type.to_string()));
        }

        sql.push_str(&format!(
            " ORDER BY bm25(messages_fts) LIMIT ?{} OFFSET ?{}",
            param_values.len() + 1,
            param_values.len() + 2
        ));

        let conn = self.conn()?;

        let count_params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let mut count_stmt = conn.prepare(&count_sql)?;
        let total: i64 = count_stmt.query_row(count_params_ref.as_slice(), |row| row.get(0))?;
        drop(count_stmt);

        param_values.push(Box::new(limit));
        param_values.push(Box::new(offset));

        let mut stmt = conn.prepare(&sql)?;
        let params_ref: Vec<&dyn rusqlite::types::ToSql> =
            param_values.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(params_ref.as_slice(), row::row_to_message)?;
        let results: Vec<Message> = rows.collect::<Result<_, _>>()?;
        debug!(query, result_count = results.len(), "search messages");
        Ok((results, total))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts5_escape_simple_word() {
        assert_eq!(fts5_escape("hello"), "\"hello\"");
    }

    #[test]
    fn fts5_escape_multiple_words() {
        assert_eq!(fts5_escape("hello world"), "\"hello\" \"world\"");
    }

    #[test]
    fn fts5_escape_at_symbol() {
        assert_eq!(fts5_escape("@MadMax"), "\"@MadMax\"");
    }

    #[test]
    fn fts5_escape_at_symbol_multi_term() {
        assert_eq!(fts5_escape("@MadMax hello"), "\"@MadMax\" \"hello\"");
    }

    #[test]
    fn fts5_escape_double_quotes_in_input() {
        assert_eq!(fts5_escape(r#"say "hi""#), "\"say\" \"\"\"hi\"\"\"");
    }

    #[test]
    fn fts5_escape_asterisk_wildcard() {
        assert_eq!(fts5_escape("test*"), "\"test*\"");
    }

    #[test]
    fn fts5_escape_dash_negation() {
        assert_eq!(fts5_escape("-excluded"), "\"-excluded\"");
    }

    #[test]
    fn fts5_escape_plus_operator() {
        assert_eq!(fts5_escape("+required"), "\"+required\"");
    }

    #[test]
    fn fts5_escape_fts5_boolean_operators() {
        assert_eq!(fts5_escape("NOT"), "\"NOT\"");
        assert_eq!(fts5_escape("AND"), "\"AND\"");
        assert_eq!(fts5_escape("OR"), "\"OR\"");
        assert_eq!(fts5_escape("NEAR"), "\"NEAR\"");
    }

    #[test]
    fn fts5_escape_boolean_in_phrase() {
        assert_eq!(fts5_escape("this AND that"), "\"this\" \"AND\" \"that\"");
    }

    #[test]
    fn fts5_escape_parentheses() {
        assert_eq!(
            fts5_escape("(hello OR world)"),
            "\"(hello\" \"OR\" \"world)\""
        );
    }

    #[test]
    fn fts5_escape_colon_column_syntax() {
        assert_eq!(fts5_escape("body:secret"), "\"body:secret\"");
    }

    #[test]
    fn fts5_escape_empty_string() {
        assert_eq!(fts5_escape(""), "\"\"");
    }

    #[test]
    fn fts5_escape_whitespace_only() {
        assert_eq!(fts5_escape("   "), "\"\"");
    }

    #[test]
    fn fts5_escape_unicode() {
        assert_eq!(fts5_escape("café résumé"), "\"café\" \"résumé\"");
    }

    #[test]
    fn fts5_escape_cjk() {
        assert_eq!(fts5_escape("会議"), "\"会議\"");
    }

    #[test]
    fn fts5_escape_emoji() {
        assert_eq!(fts5_escape("📄 report"), "\"📄\" \"report\"");
    }

    #[test]
    fn fts5_escape_curly_braces() {
        assert_eq!(fts5_escape("{hello}"), "\"{hello}\"");
    }

    #[test]
    fn fts5_escape_carets() {
        assert_eq!(fts5_escape("^prefix"), "\"^prefix\"");
    }

    #[test]
    fn fts5_escape_sql_injection_attempt() {
        assert_eq!(
            fts5_escape("'; DROP TABLE messages; --"),
            "\"';\" \"DROP\" \"TABLE\" \"messages;\" \"--\""
        );
    }

    #[test]
    fn fts5_escape_fts5_injection_via_quotes() {
        assert_eq!(fts5_escape(r#"" OR body:*"#), "\"\"\"\" \"OR\" \"body:*\"");
    }

    #[test]
    fn fts5_escape_near_with_distance() {
        assert_eq!(fts5_escape("NEAR(a b, 5)"), "\"NEAR(a\" \"b,\" \"5)\"");
    }

    #[test]
    fn fts5_escape_preserves_multiple_spaces_as_single_separator() {
        assert_eq!(fts5_escape("hello    world"), "\"hello\" \"world\"");
    }
}
