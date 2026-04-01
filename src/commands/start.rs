use std::sync::Arc;

use tokio::sync::Mutex;

use crate::data::Database;

use super::{CmdContext, MsgTarget, MsgText, update_response};

pub async fn start(
    _db: Arc<Mutex<Database>>,
    ctx: CmdContext,
) -> Result<(), teloxide::RequestError> {
    let target = &mut MsgTarget::new(ctx.chat_id, ctx.message_id);
    let msg = tr!("start_message");
    update_response(&ctx.bot, target, MsgText::html(msg)).await?;
    Ok(())
}
