use std::sync::Arc;

use teloxide::{payloads::SendDocumentSetters, requests::Requester, types::ReplyParameters};
use tokio::sync::Mutex;

use crate::data::Database;
use crate::opml::into_opml;

use super::{CmdContext, MsgTarget, MsgText, check_channel_permission, update_response};

pub async fn export(
    db: Arc<Mutex<Database>>,
    ctx: CmdContext,
) -> Result<(), teloxide::RequestError> {
    let chat_id = ctx.chat_id;
    let channel = ctx.text.trim().to_string();
    let mut target_id = chat_id;
    let target = &mut MsgTarget::new(chat_id, ctx.message_id);

    if !channel.is_empty() {
        let user_id = match ctx.from.as_ref() {
            Some(u) => u.id,
            None => return Ok(()),
        };
        let channel_id = check_channel_permission(&ctx.bot, user_id, &channel, target).await?;
        if channel_id.is_none() {
            return Ok(());
        }
        target_id = channel_id.unwrap();
    }

    let feeds = db.lock().await.subscribed_feeds(target_id.0);
    if feeds.is_none() {
        update_response(
            &ctx.bot,
            target,
            MsgText::plain(tr!("subscription_list_empty")),
        )
        .await?;
        return Ok(());
    }
    let opml = into_opml(feeds.unwrap());

    ctx.bot
        .send_document(
            chat_id,
            teloxide::types::InputFile::memory(opml.into_bytes()).file_name("feeds.opml"),
        )
        .reply_parameters(ReplyParameters::new(ctx.message_id))
        .await?;
    Ok(())
}
