use std::future::Future;
use std::pin::Pin;
use std::process;
use std::sync::Arc;
use std::task::{Poll, Context};
use std::thread;

use arc_swap::ArcSwapOption;
use futures_util::task::AtomicWaker;
use backtrace::Backtrace;


static CUR_INTERRUPT: ArcSwapOption<SignalState> = ArcSwapOption::const_empty();

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Signal {
    Interrupt,
    Term,
    Hup,
}

type SigMask = u8;

#[derive(Debug, thiserror::Error)]
#[error("interrupted")]
pub struct InterruptError(pub Signal);

pub struct Interrupt {
    event: Arc<Event>,
}

pub struct SignalState {
    backtrace: Backtrace,
    event: Arc<Event>,
    signals: SigMask,
}

struct Event {
    first: crossbeam_utils::atomic::AtomicCell<Option<Signal>>,
    last: crossbeam_utils::atomic::AtomicCell<Option<Signal>>,
    waker: AtomicWaker,
}

struct EventWait<'a>(&'a Event);

impl Event {
    fn new() -> Self {
        Event {
            first: crossbeam_utils::atomic::AtomicCell::new(None),
            last: crossbeam_utils::atomic::AtomicCell::new(None),
            waker: AtomicWaker::new(),
        }
    }
    fn set(&self, sig: Signal) {
        self.first.compare_exchange(None, Some(sig)).ok();
        self.last.store(Some(sig));
        self.waker.wake()
    }
    fn wait(&self) -> EventWait {
        EventWait(&*self)
    }
}

#[cfg(unix)]
pub fn exit_on(signal: Signal) -> ! {
    let id = signal.to_unix();
    if signal == Signal::Interrupt {
        log::warn!("Exiting due to interrupt");
    } else {
        log::warn!("Exiting on signal {}",
            signal_hook::low_level::signal_name(id).unwrap_or("<unknown>"));
    }
    process::exit(128 + id);
}

#[cfg(windows)]
pub fn exit_on(_signal: Signal) -> ! {
    _win_exit_on_interrupt();
}

#[allow(dead_code)]
fn _win_exit_on_interrupt() -> ! {
    log::warn!("Exiting due to interrupt");
    process::exit(128 + 2);
}

pub fn init_signals() {
    #[cfg(windows)]
    ctrlc::set_handler(move || {
        if let Some(state) = CUR_INTERRUPT.load_full() {
            state.event.set(Signal::Interrupt);
        } else {
            exit_on(Signal::Interrupt);
        }
    }).expect("Ctrl+C handler can be set");

    #[cfg(unix)]
    thread::spawn(|| {
        use signal_hook::iterator::Signals;
        use signal_hook::consts::signal::{SIGINT, SIGHUP, SIGTERM};

        let mut signals = Signals::new(&[SIGINT, SIGHUP, SIGTERM])
            .expect("signals initialized");
        for signal in signals.into_iter() {
            if let Some(state) = CUR_INTERRUPT.load_full()  {
                if let Some(sig) = Signal::from_unix(signal) {
                    if sig.as_bit() & state.signals != 0 {
                        state.event.set(Signal::from_unix(signal)
                            .expect("known signal"));
                    } else {
                        exit_on(sig);
                    }
                }
            }
        }
    });
}

impl Interrupt {
    pub fn ctrl_c() -> Interrupt {
        Interrupt::new(Signal::Interrupt.as_bit())
    }
    pub fn term() -> Interrupt {
        Interrupt::new(Signal::all_bits())
    }
    fn new(signals: SigMask) -> Interrupt {
        let event = Arc::new(Event::new());
        let new = Arc::new(SignalState {
            backtrace: Backtrace::new_unresolved(),
            event: event.clone(),
            signals,
        });
        let old = CUR_INTERRUPT.compare_and_swap(&None::<Arc<_>>, Some(new));
        if let Some(state) = &*old {
            let mut old_bt = state.backtrace.clone();
            old_bt.resolve();
            panic!("Second Interrupt created simutlaneously.\n\n\
                Previous was created at:\n{:?}", old_bt);
        };
        Interrupt { event }
    }
    pub async fn wait_result<T>(&self) -> anyhow::Result<T> {
        Err(InterruptError(self.event.wait().await).into())
    }
    pub async fn wait(&self) -> Signal {
        self.event.wait().await
    }
    pub fn exit_if_occurred(&self) {
        if let Some(sig) = self.event.first.load() {
            exit_on(sig);
        }
    }
}

impl Drop for Interrupt {
    fn drop(&mut self) {
        let old = CUR_INTERRUPT.swap(None::<Arc<_>>).expect("Interrupt set");
        if let Some(sig) = old.event.last.load() {
            exit_on(sig);
        }
    }
}

impl Future for EventWait<'_> {
    type Output = Signal;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Signal> {
        // quick check to avoid registration if already done.
        if let Some(sig) = self.0.last.swap(None) {
            return Poll::Ready(sig);
        }

        self.0.waker.register(cx.waker());

        // Need to check condition **after** `register` to avoid a race
        // condition that would result in lost notifications.
        if let Some(sig) = self.0.last.swap(None) {
            Poll::Ready(sig)
        } else {
            Poll::Pending
        }
    }
}

impl Signal {
    fn all_bits() -> SigMask {
        return 0b111;
    }
    fn as_bit(&self) -> SigMask {
        match self {
            Signal::Interrupt => 0b001,
            Signal::Term      => 0b010,
            Signal::Hup       => 0b100,
        }
    }
}

#[cfg(unix)]
impl Signal {
    fn to_unix(&self) -> i32 {
        use signal_hook::consts::signal::*;

        match self {
            Signal::Interrupt => SIGINT,
            Signal::Term => SIGTERM,
            Signal::Hup => SIGHUP,
        }
    }
    fn from_unix(sig: i32) -> Option<Self> {
        use signal_hook::consts::signal::*;

        match sig {
             SIGINT => Some(Signal::Interrupt),
             SIGTERM => Some(Signal::Term),
             SIGHUP => Some(Signal::Hup),
             _ => None,
        }
    }
}
