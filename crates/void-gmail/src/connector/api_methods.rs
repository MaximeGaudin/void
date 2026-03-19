use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use tracing::debug;
use tracing::warn;

use crate::api::GmailApiClient;
use crate::auth;

use super::GmailConnector;

impl GmailConnector {
    pub(crate) async fn get_client(&self) -> anyhow::Result<GmailApiClient> {
        let token_path = self.token_path();
        let mut cache = auth::TokenCache::load(&token_path)?;

        let is_expired = cache
            .expires_at
            .map(|exp| chrono::Utc::now().timestamp() >= exp - 60)
            .unwrap_or(true);

        if is_expired {
            debug!(config_id = %self.config_id, "refreshing access token");
            if let Some(ref refresh_token) = cache.refresh_token {
                let creds = auth::load_client_credentials(self.credentials_file.as_deref())?;
                let http = crate::api::build_http_client();
                cache = auth::refresh_access_token(&http, &creds, refresh_token).await?;
                cache.save(&token_path)?;
            } else {
                anyhow::bail!("token expired and no refresh token available. Run `void setup`");
            }
        } else {
            debug!(config_id = %self.config_id, "token fresh, reusing");
        }

        Ok(GmailApiClient::new(&cache.access_token))
    }

    pub async fn search_api(
        &self,
        query: &str,
        max_results: u32,
    ) -> anyhow::Result<Vec<crate::api::GmailMessage>> {
        let api = self.get_client().await?;
        let resp = api
            .list_messages(max_results, None, None, Some(query))
            .await?;
        let mut messages = Vec::new();
        if let Some(refs) = resp.messages {
            for r in &refs {
                match api.get_message(&r.id).await {
                    Ok(msg) => messages.push(msg),
                    Err(e) => warn!(message_id = %r.id, "failed to fetch: {e}"),
                }
            }
        }
        Ok(messages)
    }

    pub async fn get_thread(&self, thread_id: &str) -> anyhow::Result<crate::api::GmailThread> {
        let api = self.get_client().await?;
        api.get_thread(thread_id).await.map_err(Into::into)
    }

    pub async fn get_attachment_data(
        &self,
        message_id: &str,
        attachment_id: &str,
    ) -> anyhow::Result<Vec<u8>> {
        let api = self.get_client().await?;
        let resp = api.get_attachment(message_id, attachment_id).await?;
        let data = resp
            .data
            .ok_or_else(|| anyhow::anyhow!("attachment has no data"))?;
        URL_SAFE_NO_PAD
            .decode(data.trim_end_matches('='))
            .map_err(|e| anyhow::anyhow!("failed to decode attachment: {e}"))
    }

    pub async fn list_labels(&self) -> anyhow::Result<Vec<crate::api::GmailLabel>> {
        let api = self.get_client().await?;
        let resp = api.list_labels().await?;
        Ok(resp.labels.unwrap_or_default())
    }

    pub async fn modify_thread_labels(
        &self,
        thread_id: &str,
        add: &[&str],
        remove: &[&str],
    ) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        api.modify_thread(thread_id, add, remove).await?;
        Ok(())
    }

    pub async fn batch_modify(
        &self,
        message_ids: &[&str],
        add: &[&str],
        remove: &[&str],
    ) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        api.batch_modify_messages(message_ids, add, remove)
            .await
            .map_err(Into::into)
    }

    pub async fn list_drafts(
        &self,
        max_results: u32,
    ) -> anyhow::Result<Vec<crate::api::GmailDraft>> {
        let api = self.get_client().await?;
        let resp = api.list_drafts(max_results).await?;
        let mut drafts = Vec::new();
        if let Some(refs) = resp.drafts {
            for r in &refs {
                match api.get_draft(&r.id).await {
                    Ok(d) => drafts.push(d),
                    Err(e) => warn!(draft_id = %r.id, "failed to fetch draft: {e}"),
                }
            }
        }
        Ok(drafts)
    }

    pub async fn create_draft(
        &self,
        to: &str,
        subject: &str,
        body: &str,
        reply_to_message_id: Option<&str>,
        thread_id: Option<&str>,
    ) -> anyhow::Result<crate::api::GmailDraft> {
        let api = self.get_client().await?;

        let mut headers = format!(
            "To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=utf-8\r\n"
        );
        if let Some(ref_id) = reply_to_message_id {
            headers.push_str(&format!(
                "In-Reply-To: {ref_id}\r\nReferences: {ref_id}\r\n"
            ));
        }
        headers.push_str(&format!("\r\n{body}"));

        let encoded = URL_SAFE_NO_PAD.encode(headers.as_bytes());
        api.create_draft(&encoded, thread_id)
            .await
            .map_err(Into::into)
    }

    pub async fn update_draft(
        &self,
        draft_id: &str,
        to: &str,
        subject: &str,
        body: &str,
    ) -> anyhow::Result<crate::api::GmailDraft> {
        let api = self.get_client().await?;

        let raw = format!(
            "To: {to}\r\nSubject: {subject}\r\nContent-Type: text/plain; charset=utf-8\r\n\r\n{body}"
        );
        let encoded = URL_SAFE_NO_PAD.encode(raw.as_bytes());
        api.update_draft(draft_id, &encoded)
            .await
            .map_err(Into::into)
    }

    pub async fn delete_draft(&self, draft_id: &str) -> anyhow::Result<()> {
        let api = self.get_client().await?;
        api.delete_draft(draft_id).await.map_err(Into::into)
    }
}
