use crate::api::UnipileClient;
use crate::error::LinkedInError;

pub(crate) async fn download_media(
    client: &UnipileClient,
    message_id: &str,
    attachment_id: &str,
) -> Result<Vec<u8>, LinkedInError> {
    client.download_attachment(message_id, attachment_id).await
}
