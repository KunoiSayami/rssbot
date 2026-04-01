use std::sync::Arc;

use teloxide::{
    payloads::{EditMessageTextSetters, SendMessageSetters},
    prelude::*,
    requests::Requester,
    types::{
        ChatId, ChatKind, LinkPreviewOptions, Message, MessageId, ParseMode, PublicChatKind,
        ReplyParameters, User, UserId,
    },
};
use tokio::sync::Mutex;

use crate::data::Database;

mod export;
mod rss;
mod start;
mod sub;
mod unsub;

/// Replaces `tbot::types::parameters::Text` — bundles text with its parse mode.
#[derive(Clone)]
pub struct MsgText {
    pub text: String,
    pub parse_mode: Option<ParseMode>,
}

impl MsgText {
    pub fn plain(s: impl Into<String>) -> Self {
        MsgText {
            text: s.into(),
            parse_mode: None,
        }
    }

    pub fn html(s: impl Into<String>) -> Self {
        MsgText {
            text: s.into(),
            parse_mode: Some(ParseMode::Html),
        }
    }
}

/// Replaces `Arc<tbot::contexts::Command>` passed to each command handler.
pub struct CmdContext {
    pub bot: Bot,
    pub chat_id: ChatId,
    pub message_id: MessageId,
    pub from: Option<User>,
    /// Everything after the command word (the args string).
    pub text: String,
}

#[derive(Debug, Copy, Clone)]
pub(super) struct MsgTarget {
    chat_id: ChatId,
    message_id: MessageId,
    first_time: bool,
}

impl MsgTarget {
    fn new(chat_id: ChatId, message_id: MessageId) -> Self {
        MsgTarget {
            chat_id,
            message_id,
            first_time: true,
        }
    }

    fn update(&mut self, message_id: MessageId) {
        self.message_id = message_id;
        self.first_time = false;
    }
}

pub async fn update_response(
    bot: &Bot,
    target: &mut MsgTarget,
    message: MsgText,
) -> Result<(), teloxide::RequestError> {
    let disable_preview = LinkPreviewOptions {
        is_disabled: true,
        url: None,
        prefer_small_media: false,
        prefer_large_media: false,
        show_above_text: false,
    };
    let msg = if target.first_time {
        let mut req = bot
            .send_message(target.chat_id, &message.text)
            .reply_parameters(ReplyParameters::new(target.message_id))
            .link_preview_options(disable_preview);
        if let Some(pm) = message.parse_mode {
            req = req.parse_mode(pm);
        }
        req.await?
    } else {
        let mut req = bot
            .edit_message_text(target.chat_id, target.message_id, &message.text)
            .link_preview_options(disable_preview);
        if let Some(pm) = message.parse_mode {
            req = req.parse_mode(pm);
        }
        req.await?
    };
    target.update(msg.id);
    Ok(())
}

pub async fn check_command(bot: &Bot, opt: &crate::Opt, msg: &Message) -> bool {
    let reply_target = &mut MsgTarget::new(msg.chat.id, msg.id);

    // Private mode
    if !opt.admin.is_empty() && !is_from_bot_admin(msg, &opt.admin) {
        eprintln!("Unauthenticated request from user/channel: {:?}", msg.from);
        return false;
    }

    if let ChatKind::Public(ref chat) = msg.chat.kind {
        match &chat.kind {
            PublicChatKind::Channel(_) => {
                let _ignore = update_response(
                    bot,
                    reply_target,
                    MsgText::plain(tr!("commands_in_private_channel")),
                )
                .await;
                return false;
            }
            PublicChatKind::Group | PublicChatKind::Supergroup(_) if opt.restricted => {
                let user_is_admin = is_from_chat_admin(bot, msg).await;
                if !user_is_admin {
                    let _ignore = update_response(
                        bot,
                        reply_target,
                        MsgText::plain(tr!("group_admin_only_command")),
                    )
                    .await;
                }
                return user_is_admin;
            }
            _ => {}
        }
    }

    true
}

fn is_from_bot_admin(msg: &Message, admins: &[i64]) -> bool {
    if let Some(user) = &msg.from {
        return admins.contains(&(user.id.0 as i64));
    }
    if let Some(chat) = &msg.sender_chat {
        return admins.contains(&chat.id.0);
    }
    false
}

async fn is_from_chat_admin(bot: &Bot, msg: &Message) -> bool {
    if let Some(sender_chat) = &msg.sender_chat {
        return sender_chat.id == msg.chat.id;
    }
    let user = match &msg.from {
        Some(u) => u,
        None => return false,
    };
    let admins = match bot.get_chat_administrators(msg.chat.id).await {
        Ok(r) => r,
        Err(_) => return false,
    };
    admins.iter().any(|m| m.user.id == user.id)
}

pub async fn check_channel_permission(
    bot: &Bot,
    user_id: UserId,
    channel: &str,
    target: &mut MsgTarget,
) -> Result<Option<ChatId>, teloxide::RequestError> {
    update_response(bot, target, MsgText::plain(tr!("verifying_channel"))).await?;

    // Use numeric ChatId for numeric strings, string for @username
    let is_numeric = channel.parse::<i64>().is_ok();
    let numeric_id = channel.parse::<i64>().map(ChatId).unwrap_or(ChatId(0));

    let chat_result = if is_numeric {
        bot.get_chat(numeric_id).await
    } else {
        bot.get_chat(channel.to_string()).await
    };

    let chat = match chat_result {
        Err(teloxide::RequestError::Api(ref e)) => {
            let msg = tr!("unable_to_find_target_channel", desc = e.to_string());
            update_response(bot, target, MsgText::plain(&msg)).await?;
            return Ok(None);
        }
        other => other?,
    };

    if !chat.is_channel() {
        update_response(bot, target, MsgText::plain(tr!("target_must_be_a_channel"))).await?;
        return Ok(None);
    }

    let admins_result = if is_numeric {
        bot.get_chat_administrators(numeric_id).await
    } else {
        bot.get_chat_administrators(channel.to_string()).await
    };

    let admins = match admins_result {
        Err(teloxide::RequestError::Api(ref e)) => {
            let msg = tr!("unable_to_get_channel_info", desc = e.to_string());
            update_response(bot, target, MsgText::plain(&msg)).await?;
            return Ok(None);
        }
        other => other?,
    };

    let user_is_admin = admins.iter().any(|m| m.user.id == user_id);
    if !user_is_admin {
        update_response(
            bot,
            target,
            MsgText::plain(tr!("channel_admin_only_command")),
        )
        .await?;
        return Ok(None);
    }

    let bot_is_admin = admins
        .iter()
        .any(|m| m.user.id == *crate::BOT_ID.get().unwrap());
    if !bot_is_admin {
        update_response(bot, target, MsgText::plain(tr!("make_bot_admin"))).await?;
        return Ok(None);
    }

    Ok(Some(chat.id))
}

pub async fn register_commands(bot: Bot, opt: Arc<crate::Opt>, db: Arc<Mutex<Database>>) {
    use teloxide::dispatching::UpdateHandler;

    let handler: UpdateHandler<Box<dyn std::error::Error + Send + Sync>> =
        Update::filter_message().endpoint(dispatch_command);

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![db, opt])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;
}

async fn dispatch_command(
    bot: Bot,
    msg: Message,
    db: Arc<Mutex<Database>>,
    opt: Arc<crate::Opt>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let text = match msg.text() {
        Some(t) if t.starts_with('/') => t,
        _ => return Ok(()),
    };

    if !check_command(&bot, &opt, &msg).await {
        return Ok(());
    }

    let (cmd_name, args) = parse_command(
        text,
        crate::BOT_NAME.get().map(String::as_str).unwrap_or(""),
    );

    let ctx = CmdContext {
        bot: bot.clone(),
        chat_id: msg.chat.id,
        message_id: msg.id,
        from: msg.from.clone(),
        text: args.to_string(),
    };

    let result: Result<(), teloxide::RequestError> = match cmd_name {
        "start" => start::start(db, ctx).await,
        "rss" => rss::rss(db, ctx).await,
        "sub" => sub::sub(db, ctx).await,
        "unsub" => unsub::unsub(db, ctx).await,
        "export" => export::export(db, ctx).await,
        _ => return Ok(()),
    };

    if let Err(e) = result {
        crate::print_error(e);
    }
    Ok(())
}

/// Strip leading `/`, drop `@botname` suffix, return (command, args).
fn parse_command<'a>(text: &'a str, bot_name: &str) -> (&'a str, &'a str) {
    let without_slash = &text[1..];
    // Find end of command name (@ or space)
    let cmd_end = without_slash
        .find(|c: char| c == '@' || c == ' ')
        .unwrap_or(without_slash.len());
    let cmd = &without_slash[..cmd_end];
    let rest = &without_slash[cmd_end..];
    // If next char is '@', skip the bot name suffix
    let rest = if rest.starts_with('@') {
        // skip "@botname"
        let after_at = &rest[1..];
        let name_end = after_at.find(|c: char| c == ' ').unwrap_or(after_at.len());
        let suffix = &after_at[..name_end];
        if suffix.eq_ignore_ascii_case(bot_name) {
            &after_at[name_end..]
        } else {
            rest
        }
    } else {
        rest
    };
    (cmd, rest.trim_start())
}
