use std::cmp;
use std::collections::HashMap;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use futures::{future::FutureExt, select_biased};
use teloxide::{
    Bot,
    payloads::SendMessageSetters,
    requests::Requester,
    types::{ChatId, LinkPreviewOptions},
};
use tokio::{
    self,
    sync::{Mutex, Notify},
    time::{self, Duration, Instant},
};
use tokio_stream::StreamExt;
use tokio_util::time::DelayQueue;

use crate::client::pull_feed;
use crate::commands::MsgText;
use crate::data::{Database, Feed, FeedUpdate};
use crate::messages::{Escape, format_large_msg};

use log::{debug, info, warn};

/// A feed is auto-disabled after failing to fetch continuously for this long.
const DISABLE_AFTER: Duration = Duration::from_secs(5 * 24 * 60 * 60);
/// A disabled feed is rechecked (if a subscriber opted in) after this long.
const RECHECK_INTERVAL: Duration = Duration::from_secs(15 * 24 * 60 * 60);

fn is_recheck_due(feed: &Feed) -> bool {
    feed.disabled_at
        .and_then(|t| t.elapsed().ok())
        .map(|elapsed| elapsed >= RECHECK_INTERVAL)
        .unwrap_or(true)
}

pub fn start(
    bot: Bot,
    db: Arc<Mutex<Database>>,
    min_interval: u32,
    max_interval: u32,
    fetch_on_start: bool,
) {
    let mut queue = FetchQueue::new();
    // TODO: Don't use interval, it can accumulate ticks
    // replace it with delay_until
    let mut interval = time::interval_at(Instant::now(), Duration::from_secs(min_interval as u64));
    let throttle = Throttle::new(min_interval as usize);
    let mut first_tick = fetch_on_start;
    tokio::spawn(async move {
        loop {
            select_biased! {
                feed = queue.next().fuse() => {
                    let feed = feed.expect("unreachable");
                    let bot = bot.clone();
                    let db = db.clone();
                    let opportunity = throttle.acquire();
                    tokio::spawn(async move {
                        opportunity.wait().await;
                        if let Err(e) = fetch_and_push_updates(bot, db, feed).await {
                            crate::print_error(e);
                        }
                    });
                }
                _ = interval.tick().fuse() => {
                    let db_guard = db.lock().await;
                    let feeds = db_guard.all_feeds();
                    info!("Scheduling {} feed(s) for fetch", feeds.len());
                    for feed in feeds {
                        if feed.disabled {
                            if is_recheck_due(&feed) && db_guard.feed_recheck_wanted(&feed) {
                                debug!("Feed '{}' ({}): due for 15-day recheck", feed.title, feed.link);
                                queue.enqueue(feed.clone(), Duration::ZERO);
                            }
                            continue;
                        }
                        let feed_interval = if first_tick {
                            0
                        } else {
                            cmp::min(
                                cmp::max(
                                    feed.ttl.map(|ttl| ttl * 60).unwrap_or_default(),
                                    min_interval,
                                ),
                                max_interval,
                            ) as u64 - 1 // after -1, we can stagger with `interval`
                        };
                        let enqueued = queue.enqueue(feed.clone(), Duration::from_secs(feed_interval));
                        debug!("Feed '{}' ({}): enqueued={enqueued}, next fetch in {}s", feed.title, feed.link, feed_interval);
                    }
                    drop(db_guard);
                    first_tick = false;
                }
            }
        }
    });
}

async fn fetch_and_push_updates(
    bot: Bot,
    db: Arc<Mutex<Database>>,
    feed: Feed,
) -> Result<(), teloxide::RequestError> {
    info!("Fetching feed '{}' ({})", feed.title, feed.link);
    let new_feed = match pull_feed(&feed.link).await {
        Ok(feed) => feed,
        Err(e) => {
            warn!("Failed to fetch '{}': {}", feed.link, e);
            if feed.disabled {
                // Recheck failed again, wait for the next recheck interval.
                db.lock().await.postpone_recheck(&feed.link);
                return Ok(());
            }
            let down_time = db.lock().await.get_or_update_down_time(&feed.link);
            if down_time.is_none() {
                // user unsubscribed while fetching the feed
                return Ok(());
            }
            if down_time.unwrap() > DISABLE_AFTER {
                db.lock().await.disable_feed(&feed.link);
                let msg = tr!(
                    "continuous_fetch_error",
                    title = Escape(&feed.title),
                    error = Escape(&e.to_user_friendly())
                );
                push_updates(&bot, &db, feed.subscribers, MsgText::html(&msg)).await?;
            }
            return Ok(());
        }
    };

    if feed.disabled {
        info!(
            "Feed '{}' ({}) recovered, re-enabling",
            feed.title, feed.link
        );
        db.lock().await.enable_feed(&feed.link);
        let msg = tr!("feed_reenabled", title = Escape(&feed.title));
        push_updates(
            &bot,
            &db,
            feed.subscribers.iter().copied(),
            MsgText::html(&msg),
        )
        .await?;
    }

    let updates = db.lock().await.update(&feed.link, new_feed);
    if updates.is_empty() {
        debug!("No updates for '{}'", feed.link);
    }
    for update in updates {
        match update {
            FeedUpdate::Items(items) => {
                info!("Pushing {} new item(s) for '{}'", items.len(), feed.title);
                let msgs =
                    format_large_msg(format!("<b>{}</b>", Escape(&feed.title)), &items, |item| {
                        let title = item.title.as_deref().unwrap_or_else(|| &feed.title);
                        let link = item.link.as_deref().unwrap_or_else(|| &feed.link);
                        format!("<a href=\"{}\">{}</a>", Escape(link), Escape(title))
                    });
                for msg in msgs {
                    push_updates(
                        &bot,
                        &db,
                        feed.subscribers.iter().copied(),
                        MsgText::html(&msg),
                    )
                    .await?;
                }
            }
            FeedUpdate::Title(new_title) => {
                info!("Feed '{}' renamed to '{new_title}'", feed.title);
                let msg = tr!(
                    "feed_renamed",
                    title = Escape(&feed.title),
                    new_title = Escape(&new_title)
                );
                push_updates(
                    &bot,
                    &db,
                    feed.subscribers.iter().copied(),
                    MsgText::html(&msg),
                )
                .await?;
            }
        }
    }
    Ok(())
}

async fn push_updates<I: IntoIterator<Item = i64>>(
    bot: &Bot,
    db: &Arc<Mutex<Database>>,
    subscribers: I,
    msg: MsgText,
) -> Result<(), teloxide::RequestError> {
    for mut subscriber in subscribers {
        'retry: for _ in 0..3 {
            let mut req = bot
                .send_message(ChatId(subscriber), &msg.text)
                .link_preview_options(LinkPreviewOptions {
                    is_disabled: true,
                    url: None,
                    prefer_small_media: false,
                    prefer_large_media: false,
                    show_above_text: false,
                });
            if let Some(pm) = msg.parse_mode {
                req = req.parse_mode(pm);
            }
            match req.await {
                Err(teloxide::RequestError::Api(ref e)) if chat_is_unavailable(&e.to_string()) => {
                    db.lock().await.delete_subscriber(subscriber);
                }
                Err(teloxide::RequestError::MigrateToChatId(new_chat_id)) => {
                    db.lock().await.update_subscriber(subscriber, new_chat_id.0);
                    subscriber = new_chat_id.0;
                    continue 'retry;
                }
                Err(teloxide::RequestError::RetryAfter(seconds)) => {
                    time::sleep(seconds.duration()).await;
                    continue 'retry;
                }
                other => {
                    other?;
                }
            }
            break 'retry;
        }
    }
    Ok(())
}

pub fn chat_is_unavailable(s: &str) -> bool {
    s.contains("Forbidden")
        || s.contains("chat not found")
        || s.contains("have no rights")
        || s.contains("need administrator rights")
}

#[derive(Default)]
struct FetchQueue {
    feeds: HashMap<String, Feed>,
    notifies: DelayQueue<String>,
    wakeup: Notify,
}

impl FetchQueue {
    fn new() -> Self {
        Self::default()
    }

    fn enqueue(&mut self, feed: Feed, delay: Duration) -> bool {
        let exists = self.feeds.contains_key(&feed.link);
        if !exists {
            self.notifies.insert(feed.link.clone(), delay);
            self.feeds.insert(feed.link.clone(), feed);
            self.wakeup.notify_waiters();
        }
        !exists
    }

    async fn next(&mut self) -> Result<Feed, time::error::Error> {
        loop {
            if let Some(feed_id) = self.notifies.next().await {
                let feed = self.feeds.remove(feed_id.get_ref()).unwrap();
                break Ok(feed);
            } else {
                self.wakeup.notified().await;
            }
        }
    }
}

struct Throttle {
    pieces: usize,
    counter: Arc<AtomicUsize>,
}

impl Throttle {
    fn new(pieces: usize) -> Self {
        Throttle {
            pieces,
            counter: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn acquire(&self) -> Opportunity {
        Opportunity {
            n: self.counter.fetch_add(1, Ordering::AcqRel) % self.pieces,
            counter: self.counter.clone(),
        }
    }
}

#[must_use = "Don't lose your opportunity"]
struct Opportunity {
    n: usize,
    counter: Arc<AtomicUsize>,
}

impl Opportunity {
    async fn wait(&self) {
        time::sleep(Duration::from_secs(self.n as u64)).await
    }
}

impl Drop for Opportunity {
    fn drop(&mut self) {
        self.counter.fetch_sub(1, Ordering::SeqCst);
    }
}
