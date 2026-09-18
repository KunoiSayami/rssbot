use std::sync::Arc;

use tokio::sync::Mutex;

use crate::data::Database;

use super::{CmdContext, MsgTarget, MsgText, update_response};

pub async fn config(
    db: Arc<Mutex<Database>>,
    ctx: CmdContext,
) -> Result<(), teloxide::RequestError> {
    let target = &mut MsgTarget::new(ctx.chat_id, ctx.message_id);
    let args = ctx.text.split_whitespace().collect::<Vec<_>>();

    let user_id = match ctx.from.as_ref() {
        Some(u) => u.id,
        None => return Ok(()),
    };

    match &*args {
        ["recheck", "on"] => {
            db.lock().await.set_recheck_enabled(user_id.0 as i64, true);
            update_response(&ctx.bot, target, MsgText::plain(tr!("recheck_enabled"))).await?;
        }
        ["recheck", "off"] => {
            db.lock().await.set_recheck_enabled(user_id.0 as i64, false);
            update_response(&ctx.bot, target, MsgText::plain(tr!("recheck_disabled"))).await?;
        }
        ["recheck"] => {
            let enabled = db.lock().await.recheck_enabled(user_id.0 as i64);
            let msg = if enabled {
                tr!("recheck_status_on")
            } else {
                tr!("recheck_status_off")
            };
            update_response(&ctx.bot, target, MsgText::plain(msg)).await?;
        }
        _ => {
            update_response(&ctx.bot, target, MsgText::html(tr!("config_how_to_use"))).await?;
        }
    }
    Ok(())
}
