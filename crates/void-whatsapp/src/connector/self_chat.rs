//! WhatsApp "Message yourself" (notes-to-self) detection and outbound routing.

use std::sync::Arc;

use tracing::info;
use wa_rs::client::Client;
use wa_rs::send::SendOptions;
use wa_rs::types::message::MessageSource;
use wa_rs::Jid;
use wa_rs_binary::jid::JidExt;
use wa_rs_proto::whatsapp::ContextInfo;

use super::send::{build_wa_message, normalize_phone, parse_jid};
use void_core::models::MessageContent;

pub const SELF_CHAT_DISPLAY_NAME: &str = "Message yourself";

/// Own WhatsApp identifiers discovered at runtime (phone JID + LID).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnIdentity {
    pub phone_jid: Option<String>,
    pub lid_jid: Option<String>,
}

impl OwnIdentity {
    pub fn update_from_connected(&mut self, phone: Option<Jid>, lid: Option<Jid>) {
        if let Some(pn) = phone {
            self.phone_jid = Some(pn.to_non_ad().to_string());
        }
        if let Some(lid) = lid {
            self.lid_jid = Some(lid.to_non_ad().to_string());
        }
    }

    pub fn update_from_message(&mut self, source: &MessageSource) {
        let sender = source.sender.to_non_ad().to_string();
        if source.sender.is_lid() {
            if self.lid_jid.is_none() {
                self.lid_jid = Some(sender);
            }
        } else if self.phone_jid.is_none() {
            self.phone_jid = Some(sender);
        }
    }

    /// Notes-to-self thread: chat JID is the account's own @lid.
    pub fn is_self_chat(&self, chat_jid: &str) -> bool {
        let Some(own_lid) = self.lid_jid.as_deref() else {
            return false;
        };
        jid_same_user(chat_jid, own_lid)
    }

    /// Whether an outbound target refers to this account (phone or LID).
    pub fn is_own_address(&self, address: &str) -> bool {
        if let Some(phone) = self.phone_jid.as_deref() {
            if jid_or_phone_same_user(address, phone) {
                return true;
            }
        }
        if let Some(lid) = self.lid_jid.as_deref() {
            if jid_same_user(address, lid) {
                return true;
            }
        }
        false
    }

    pub fn should_route_as_self_chat(&self, target: &str) -> bool {
        self.is_self_chat(target) || self.is_own_address(target)
    }

    pub fn self_chat_jid(&self) -> Option<&str> {
        self.lid_jid.as_deref()
    }

    pub async fn enrich_from_client(&self, client: &Client) -> Self {
        let mut identity = self.clone();
        if identity.lid_jid.is_none() {
            if let Some(lid) = client.get_lid().await {
                identity.lid_jid = Some(lid.to_non_ad().to_string());
            }
        }
        if identity.phone_jid.is_none() {
            if let Some(pn) = client.get_pn().await {
                identity.phone_jid = Some(pn.to_non_ad().to_string());
            }
        }
        identity
    }
}

/// Attributes for an inbound notes-to-self stanza (kept for reference / future use).
#[allow(dead_code)]
pub fn apply_self_chat_stanza_attrs(
    stanza: &mut wa_rs_binary::node::Node,
    own_lid: &Jid,
    own_pn: &Jid,
) {
    stanza
        .attrs
        .insert("recipient".to_string(), own_lid.to_non_ad().to_string());
    stanza.attrs.insert(
        "peer_recipient_pn".to_string(),
        own_pn.to_non_ad().to_string(),
    );
}

pub async fn send_self_chat_message(
    client: &Arc<Client>,
    identity: &OwnIdentity,
    content: MessageContent,
    context_info: Option<ContextInfo>,
) -> anyhow::Result<String> {
    let identity = identity.enrich_from_client(client).await;
    let own_pn = identity.phone_jid.as_deref().ok_or_else(|| {
        anyhow::anyhow!("WhatsApp phone JID not available for notes-to-self send")
    })?;
    let own_pn = parse_jid(own_pn)?;

    let msg = match &content {
        MessageContent::File { .. } => {
            anyhow::bail!("File attachments are not yet supported for notes-to-self sends")
        }
        _ => build_wa_message(&content, context_info)?,
    };

    info!(
        own_pn = %own_pn,
        "sending WhatsApp notes-to-self message"
    );

    // Use the same DM path as wa-rs for regular sends. The custom LID-targeted
    // stanza from 0.10.1 encrypted only for linked-device LIDs and never reached
    // the primary phone.
    let msg_id = client
        .send_message_with_options(own_pn, msg, SendOptions::default())
        .await?;
    Ok(msg_id)
}

fn jid_same_user(a: &str, b: &str) -> bool {
    match (parse_jid(a), parse_jid(b)) {
        (Ok(left), Ok(right)) => left.is_same_user_as(&right),
        _ => false,
    }
}

fn jid_or_phone_same_user(address: &str, phone_jid: &str) -> bool {
    if jid_same_user(address, phone_jid) {
        return true;
    }
    let normalized = normalize_phone(address);
    parse_jid(phone_jid)
        .ok()
        .is_some_and(|jid| jid.user == normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wa_rs::types::message::MessageSource;

    fn identity() -> OwnIdentity {
        OwnIdentity {
            phone_jid: Some("33651090627@s.whatsapp.net".into()),
            lid_jid: Some("94004066660357@lid".into()),
        }
    }

    #[test]
    fn is_self_chat_matches_own_lid() {
        let id = identity();
        assert!(id.is_self_chat("94004066660357@lid"));
        assert!(!id.is_self_chat("33651090627@s.whatsapp.net"));
        assert!(!id.is_self_chat("120363@g.us"));
    }

    #[test]
    fn is_own_address_accepts_phone_and_lid() {
        let id = identity();
        assert!(id.is_own_address("33651090627"));
        assert!(id.is_own_address("+33651090627"));
        assert!(id.is_own_address("33651090627@s.whatsapp.net"));
        assert!(id.is_own_address("94004066660357@lid"));
        assert!(!id.is_own_address("33612345678"));
    }

    #[test]
    fn should_route_own_phone_to_self_chat() {
        let id = identity();
        assert!(id.should_route_as_self_chat("33651090627"));
        assert!(id.should_route_as_self_chat("94004066660357@lid"));
    }

    #[test]
    fn update_from_message_records_lid_sender() {
        let mut id = OwnIdentity::default();
        let source = MessageSource {
            chat: parse_jid("94004066660357@lid").unwrap(),
            sender: parse_jid("94004066660357@lid").unwrap(),
            is_from_me: true,
            ..Default::default()
        };
        id.update_from_message(&source);
        assert_eq!(id.lid_jid.as_deref(), Some("94004066660357@lid"));
    }

    #[test]
    fn apply_self_chat_stanza_attrs_sets_recipient_fields() {
        let own_lid = parse_jid("94004066660357@lid").unwrap();
        let own_pn = parse_jid("33651090627@s.whatsapp.net").unwrap();
        let mut stanza =
            wa_rs_binary::node::Node::new("message", wa_rs_binary::node::Attrs::new(), None);
        apply_self_chat_stanza_attrs(&mut stanza, &own_lid, &own_pn);
        assert_eq!(
            stanza.attrs.get("recipient").and_then(|v| v.as_str()),
            Some("94004066660357@lid")
        );
        assert_eq!(
            stanza
                .attrs
                .get("peer_recipient_pn")
                .and_then(|v| v.as_str()),
            Some("33651090627@s.whatsapp.net")
        );
    }
}
