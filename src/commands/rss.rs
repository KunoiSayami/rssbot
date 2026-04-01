use std::sync::Arc;

use either::Either;
use pinyin::{Pinyin, ToPinyin};
use teloxide::{
    payloads::SendMessageSetters,
    requests::Requester,
    types::{LinkPreviewOptions, ParseMode, ReplyParameters},
};
use tokio::sync::Mutex;

use crate::data::Database;
use crate::messages::{format_large_msg, Escape};

use super::{check_channel_permission, update_response, CmdContext, MsgTarget, MsgText};

pub async fn rss(db: Arc<Mutex<Database>>, ctx: CmdContext) -> Result<(), teloxide::RequestError> {
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
    let mut msgs = if let Some(mut feeds) = feeds {
        feeds.sort_by_cached_key(|feed| {
            feed.title
                .chars()
                .map(|c| {
                    c.to_pinyin()
                        .map(Pinyin::plain)
                        .map(Either::Right)
                        .unwrap_or_else(|| Either::Left(c))
                })
                .collect::<Vec<Either<char, &str>>>()
        });
        format_large_msg(tr!("subscription_list").to_string(), &feeds, |feed| {
            format!(
                "<a href=\"{}\">{}</a>",
                Escape(&feed.link),
                Escape(&feed.title)
            )
        })
    } else {
        vec![tr!("subscription_list_empty").to_string()]
    };

    let first_msg = msgs.remove(0);
    update_response(&ctx.bot, target, MsgText::html(&first_msg)).await?;

    let mut prev_msg = target.message_id;
    for msg in msgs {
        let sent = ctx
            .bot
            .send_message(chat_id, &msg)
            .reply_parameters(ReplyParameters::new(prev_msg))
            .link_preview_options(LinkPreviewOptions {
                is_disabled: true,
                url: None,
                prefer_small_media: false,
                prefer_large_media: false,
                show_above_text: false,
            })
            .parse_mode(ParseMode::Html)
            .await?;
        prev_msg = sent.id;
    }
    Ok(())
}
