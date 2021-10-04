use std::future::Future;
use std::pin::Pin;
use std::process;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::task::{Poll, Context};

use arc_swap::ArcSwapOption;
use futures_util::task::AtomicWaker;
use backtrace::Backtrace;


static CUR_CTRLC: ArcSwapOption<CtrlCState> = ArcSwapOption::const_empty();


#[derive(Debug, thiserror::Error)]
#[error("interrupted with Ctrl+C")]
pub struct InterruptError;

#[derive(Debug, thiserror::Error)]
#[error("termination signal received")]
pub struct TermError;

pub struct CtrlC {
    event: Arc<Event>,
}

pub struct CtrlCState {
    backtrace: Backtrace,
    event: Arc<Event>,
}

struct Event {
    value: AtomicBool,
    waker: AtomicWaker,
}

struct EventWait<'a>(&'a Event);

impl Event {
    fn new() -> Self {
        Event {
            value: AtomicBool::new(false),
            waker: AtomicWaker::new(),
        }
    }
    fn is_set(&self) -> bool {
        self.value.load(Relaxed)
    }
    fn set(&self) {
        self.value.store(true, Relaxed);
        self.waker.wake()
    }
    fn wait(&self) -> EventWait {
        EventWait(&*self)
    }
}

fn exit_on_signal() {
    log::warn!("Exiting due to interrupt");
    process::exit(143); // 128 + SIGINT signal convention
}

pub fn init_signals() {
    ctrlc::set_handler(move || {
        if let Some(state) = CUR_CTRLC.load_full() {
            state.event.set();
        } else {
            exit_on_signal();
        }
    }).expect("Ctrl+C handler can be set");
}

fn clear_ctrl_c() {
    let old = CUR_CTRLC.swap(None::<Arc<_>>).expect("CtrlC set");
    if old.event.is_set() {
        exit_on_signal();
    }
}

impl CtrlC {
    pub fn new() -> CtrlC {
        let event = Arc::new(Event::new());
        let new = Arc::new(CtrlCState {
            backtrace: Backtrace::new(),
            event: event.clone(),
        });
        let old = CUR_CTRLC.compare_and_swap(&None::<Arc<_>>, Some(new));
        if let Some(state) = &*old {
            panic!("Second CtrlC created simutlaneously.\n\n\
                Previous was created at:\n{:?}", state.backtrace);
        };
        CtrlC { event }
    }
    pub async fn wait_result<T>(&self) -> anyhow::Result<T> {
        self.event.wait().await;
        Err(InterruptError.into())
    }
    pub async fn wait(&self) {
        self.event.wait().await;
    }
}

impl Drop for CtrlC {
    fn drop(&mut self) {
        clear_ctrl_c();
    }
}

impl Future for EventWait<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // quick check to avoid registration if already done.
        if self.0.value.load(Relaxed) {
            return Poll::Ready(());
        }

        self.0.waker.register(cx.waker());

        // Need to check condition **after** `register` to avoid a race
        // condition that would result in lost notifications.
        if self.0.value.load(Relaxed) {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}
