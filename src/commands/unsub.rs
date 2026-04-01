use std::sync::Arc;

use tokio::sync::Mutex;

use crate::data::Database;
use crate::messages::Escape;

use super::{check_channel_permission, update_response, CmdContext, MsgTarget, MsgText};

pub async fn unsub(
    db: Arc<Mutex<Database>>,
    ctx: CmdContext,
) -> Result<(), teloxide::RequestError> {
    let chat_id = ctx.chat_id;
    let args = ctx.text.split_whitespace().collect::<Vec<_>>();
    let mut target_id = chat_id;
    let target = &mut MsgTarget::new(chat_id, ctx.message_id);
    let feed_url;

    match &*args {
        [url] => feed_url = url,
        [channel, url] => {
            let user_id = match ctx.from.as_ref() {
                Some(u) => u.id,
                None => return Ok(()),
            };
            let channel_id = check_channel_permission(&ctx.bot, user_id, channel, target).await?;
            if channel_id.is_none() {
                return Ok(());
            }
            target_id = channel_id.unwrap();
            feed_url = url;
        }
        [..] => {
            let msg = tr!("unsub_how_to_use");
            update_response(&ctx.bot, target, MsgText::plain(msg)).await?;
            return Ok(());
        }
    };

    let msg = if let Some(feed) = db.lock().await.unsubscribe(target_id.0, feed_url) {
        tr!(
            "unsubscription_succeeded",
            link = Escape(&feed.link),
            title = Escape(&feed.title)
        )
    } else {
        tr!("unsubscribed_from_rss").into()
    };
    update_response(&ctx.bot, target, MsgText::html(&msg)).await?;
    Ok(())
}
