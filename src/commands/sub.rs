use std::sync::Arc;

use tokio::sync::Mutex;

use crate::client::pull_feed;
use crate::data::Database;
use crate::messages::Escape;

use super::{CmdContext, MsgTarget, MsgText, check_channel_permission, update_response};

pub async fn sub(db: Arc<Mutex<Database>>, ctx: CmdContext) -> Result<(), teloxide::RequestError> {
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
                None => {
                    // anonymous channel post cannot manage channels
                    return Ok(());
                }
            };
            let channel_id = check_channel_permission(&ctx.bot, user_id, channel, target).await?;
            if channel_id.is_none() {
                return Ok(());
            }
            target_id = channel_id.unwrap();
            feed_url = url;
        }
        [..] => {
            let msg = tr!("sub_how_to_use");
            update_response(&ctx.bot, target, MsgText::plain(msg)).await?;
            return Ok(());
        }
    };

    if db.lock().await.is_subscribed(target_id.0, feed_url) {
        update_response(&ctx.bot, target, MsgText::plain(tr!("subscribed_to_rss"))).await?;
        return Ok(());
    }

    if cfg!(feature = "hosted-by-iovxw") && db.lock().await.all_feeds().len() >= 1500 {
        let msg = tr!("subscription_rate_limit");
        update_response(&ctx.bot, target, MsgText::markdown(msg)).await?;
        return Ok(());
    }

    update_response(
        &ctx.bot,
        target,
        MsgText::plain(tr!("processing_please_wait")),
    )
    .await?;

    let msg = match pull_feed(feed_url).await {
        Ok(feed) => {
            if db.lock().await.subscribe(target_id.0, feed_url, &feed) {
                tr!(
                    "subscription_succeeded",
                    link = Escape(&feed.link),
                    title = Escape(&feed.title)
                )
            } else {
                tr!("subscribed_to_rss").into()
            }
        }
        Err(e) => tr!("subscription_failed", error = Escape(&e.to_user_friendly())),
    };
    update_response(&ctx.bot, target, MsgText::html(&msg)).await?;
    Ok(())
}
