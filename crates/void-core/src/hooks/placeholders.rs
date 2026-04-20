use crate::models::Message;

pub fn expand_placeholders_public(template: &str, msg: Option<&Message>) -> String {
    expand_placeholders(template, msg)
}

pub(crate) fn expand_placeholders(template: &str, msg: Option<&Message>) -> String {
    let now = chrono::Utc::now();
    let mut result = template
        .replace(
            "{now}",
            &now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        )
        .replace("{today}", &now.format("%Y-%m-%d").to_string());

    if let Some(msg) = msg {
        result = result.replace("{message_id}", &msg.id);
        result = result.replace("{connector}", &msg.connector);
        result = result.replace("{connection_id}", &msg.connection_id);
        if let Ok(json) = serde_json::to_string_pretty(msg) {
            result = result.replace("{message}", &json);
        }
    }

    result
}
