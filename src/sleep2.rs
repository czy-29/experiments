use std::{
    future::Future,
    pin::Pin,
    sync::{
        mpsc::{channel, Receiver, RecvTimeoutError, Sender},
        Arc, RwLock,
    },
    task::{Context, Poll, Waker},
    thread::{self, JoinHandle},
    time::Duration,
};

#[derive(Debug)]
struct SleepCanceler(#[allow(dead_code)] Sender<()>);

#[derive(Debug)]
struct SleepWaiter {
    dur: Duration,
    recv: Receiver<()>,
}

impl SleepWaiter {
    fn wait(&self) -> bool {
        match self.recv.recv_timeout(self.dur) {
            Ok(_) => unreachable!(),
            Err(RecvTimeoutError::Timeout) => true,
            Err(RecvTimeoutError::Disconnected) => false,
        }
    }
}

fn sleep_channel(dur: Duration) -> (SleepCanceler, SleepWaiter) {
    let (send, recv) = channel();
    (SleepCanceler(send), SleepWaiter { dur, recv })
}

#[derive(Debug)]
struct SleepHandles {
    waker: Option<Waker>,
    thread: Option<JoinHandle<()>>,
    _canceler: SleepCanceler,
}

#[derive(Debug)]
enum SleepState {
    Init,
    Start(SleepHandles),
    Canceled,
    Elapsed(Option<JoinHandle<()>>),
}

impl SleepState {
    fn is_elapsed(&self) -> bool {
        matches!(*self, SleepState::Elapsed(_))
    }

    fn start(canceler: SleepCanceler, waker: Waker, thread: JoinHandle<()>) -> Self {
        Self::Start(SleepHandles {
            waker: Some(waker),
            thread: Some(thread),
            _canceler: canceler,
        })
    }
}

#[derive(Debug, Clone)]
struct Inner(Arc<RwLock<SleepState>>);

impl Inner {
    fn init() -> Self {
        Self(Arc::new(RwLock::new(SleepState::Init)))
    }

    fn is_elapsed(&self) -> bool {
        self.0.read().unwrap().is_elapsed()
    }

    fn cancel(&self) {
        let mut thread = None;

        {
            let mut guard = self.0.write().unwrap();

            if let SleepState::Start(handles) = &mut *guard {
                thread = handles.thread.take();
                *guard = SleepState::Canceled;
            } else if let SleepState::Elapsed(thread_handle) = &mut *guard {
                thread = thread_handle.take();
            }
        }

        if let Some(thread) = thread {
            thread.join().ok();
        }
    }

    fn elapsed(&self) {
        let mut guard = self.0.write().unwrap();

        if let SleepState::Start(handles) = &mut *guard {
            let thread = handles.thread.take();
            handles.waker.take().map(Waker::wake);
            *guard = SleepState::Elapsed(thread);
        }
    }
}

#[derive(Debug)]
pub struct Sleep {
    dur: Duration,
    state: Inner,
}

impl Sleep {
    pub fn dur(&self) -> Duration {
        self.dur
    }

    pub fn is_elapsed(&self) -> bool {
        self.state.is_elapsed()
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        self.state.cancel();
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut guard = self.state.0.write().unwrap();

        match &mut *guard {
            SleepState::Canceled => unreachable!(),
            SleepState::Elapsed(_) => Poll::Ready(()),
            SleepState::Start(handles) => {
                if let Some(waker) = &mut handles.waker {
                    waker.clone_from(cx.waker());
                }

                Poll::Pending
            }
            SleepState::Init => {
                let (canceler, waiter) = sleep_channel(self.dur);
                let state = self.state.clone();
                let waker = cx.waker().clone();
                let thread = thread::spawn(move || {
                    if waiter.wait() {
                        state.elapsed();
                    }
                });

                *guard = SleepState::start(canceler, waker, thread);
                Poll::Pending
            }
        }
    }
}

pub fn sleep(dur: Duration) -> Sleep {
    Sleep {
        dur,
        state: Inner::init(),
    }
}
