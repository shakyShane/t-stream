use core::pin::Pin;
use core::task::{Context, Poll};
use std::fmt::Debug;
use std::time::Duration;

use futures::{Future, Stream};
use pin_project_lite::pin_project;
use tokio::sync::{mpsc};
use tokio::time::{Instant, Sleep};
use tokio::time::sleep;
use tokio_stream::{StreamExt};
use tokio_stream::wrappers::ReceiverStream;
use tracing::{info, trace};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

pin_project! {
    #[must_use = "streams do nothing unless polled"]
    pub struct Debounce<St: Stream> {
        #[pin]
        value: St,
        #[pin]
        now: Instant,
        #[pin]
        delay: Sleep,
        #[pin]
        debounce_time: Duration,
        #[pin]
        last_state: Option<St::Item>
    }
}

pub trait StreamOpsExt: Stream {
    fn debounce(self, debounce_time: Duration, now: Instant) -> Debounce<Self>
    where
        Self: Sized + Unpin,
    {
        Debounce::new(self, debounce_time, now)
    }
}

impl<T: ?Sized> StreamOpsExt for T where T: Stream {}

impl<St> Debounce<St>
where
    St: Stream + Unpin,
{
    #[allow(dead_code)]
    fn new(stream: St, debounce_time: Duration, now: Instant) -> Debounce<St> {
        Debounce {
            value: stream,
            delay: tokio::time::sleep(debounce_time),
            debounce_time,
            last_state: None,
            now
        }
    }
}

impl<St, Item> Stream for Debounce<St>
where
    St: Stream<Item = Item>,
    Item: Clone + Unpin + Debug + 'static,
{
    type Item = St::Item;
    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let mut me = self.project();
        trace!("+ polled!");

        match me.value.poll_next(cx) {
            Poll::Ready(Some(v)) => {
                trace!("recorded a child value");
                *me.last_state = Some(v);

                trace!("resetting the deadline to be `debounce_time` from `now`");
                let dur = me.debounce_time.clone();
                me.delay.as_mut().reset(Instant::now() + dur);

                trace!("wake to ensure we're polled again");
                cx.waker().wake_by_ref();
                return Poll::Pending;
            }
            Poll::Ready(None) => {
                trace!("child ended, nothing more to do?");
            }
            Poll::Pending => {
                trace!("child was pending, nothing to do");
            }
        }

        match me.delay.poll(cx) {
            Poll::Ready(_) => {
                trace!("timer elapsed");
                match (*me.last_state).clone() {
                    Some(v) => {
                        *me.last_state = None;
                        Poll::Ready(Some(v))
                    },
                    None => {
                        Poll::Ready(None)
                    }
                }
            }
            Poll::Pending => {
                Poll::Pending
            }
        }
    }
}

#[tokio::main]
async fn main() {

    let fmt_layer = tracing_subscriber::fmt::layer();
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "t_stream=info".into()),
        )
        .with(fmt_layer)
        .init();

    let (tx, rx) = mpsc::channel::<&str>(10);
    let handle = tokio::spawn(async move {
        let events = ["A", "B", "C", "D", "E", "F"];

        // 6 events all happening together
        for evt in events {
            tx.send(evt).await.unwrap();
        }

        // a gap in events, just under the debounce duration
        sleep(Duration::from_millis(800)).await;

        tx.send("G").await.unwrap();
        tx.send("H").await.unwrap();
        tx.send("I").await.unwrap();
        tx.send("J").await.unwrap();

        // drop the sender to complete the stream
        drop(tx);
    });

    let start_time = Instant::now();
    let mut stream = Box::pin(
        ReceiverStream::new(rx)
            .debounce(Duration::from_millis(1000), Instant::now())
            .collect::<Vec<_>>()
    );

    let results = stream.await;
    let end_time = Instant::now();
    let total_duration = end_time.checked_duration_since(start_time).expect("checked");

    info!(?results);
    info!(?total_duration, "overall duration");

    assert!(handle.await.is_ok());
    assert_eq!(vec!["J"], results);
}
