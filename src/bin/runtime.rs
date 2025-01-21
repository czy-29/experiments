use std::{
    future::Future,
    pin::Pin,
    sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender},
    task::{Context, Poll},
    thread::{self, JoinHandle},
    time::Duration,
};

struct WithCancelSignal<F, C> {
    future: F,
    cancel: C,
}

impl<F, C> Future for WithCancelSignal<F, C>
where
    F: Future + Unpin,
    C: Future + Unpin,
{
    type Output = Result<F::Output, C::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Poll::Ready(o) = Pin::new(&mut self.future).poll(cx) {
            return Poll::Ready(Ok(o));
        }

        if let Poll::Ready(o) = Pin::new(&mut self.cancel).poll(cx) {
            return Poll::Ready(Err(o));
        }

        Poll::Pending
    }
}

trait FutureExt: Future + Unpin + Sized {
    fn with_cancel_signal<C: Future + Unpin>(self, cancel: C) -> WithCancelSignal<Self, C> {
        WithCancelSignal {
            future: self,
            cancel,
        }
    }
}

impl<T: Future + Unpin + Sized> FutureExt for T {}

struct SleepHandles {
    thread: JoinHandle<()>,
    elapsed: Receiver<()>,
    sleep_cancel: Sender<()>,
}

pub struct Sleep {
    dur: Duration,
    handles: Option<SleepHandles>,
    done: bool,
}

impl Drop for Sleep {
    fn drop(&mut self) {
        if let Some(handles) = self.handles.take() {
            drop(handles.elapsed);
            drop(handles.sleep_cancel);
            handles.thread.join().ok();
        }
    }
}

impl Future for Sleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &self.handles {
            Some(handles) => if self.done {
                panic!("cannot continue polling after sleep returns");
            } else {
                match handles.elapsed.try_recv() {
                    Err(_) => Poll::Pending,
                    Ok(_) => {
                        self.done = true;
                        Poll::Ready(())
                    }
                }
            },
            None => {
                let (notify, elapsed) = channel();
                let (sleep_cancel, sleep_wait) = channel();
                let dur = self.dur;
                let waker = cx.waker().clone();
                let thread = thread::spawn(move || {
                    match sleep_wait.recv_timeout(dur) {
                        Ok(_) => unreachable!(),
                        Err(RecvTimeoutError::Disconnected) => (),
                        Err(RecvTimeoutError::Timeout) => {
                            if notify.send(()).is_ok() {
                                waker.wake();
                            }
                        }
                    }
                });
                let handles = SleepHandles {
                    thread,
                    elapsed,
                    sleep_cancel,
                };

                self.handles = Some(handles);
                Poll::Pending
            }
        }
    }
}

pub fn sleep(dur: Duration) -> Sleep {
    Sleep { dur, handles: None, done: false }
}

// task, spawn(poll_immediate: Option<bool>), runtime
// rand, with_cancel_signal(tokio::signal::ctrlc)

fn main() {
    println!("Hello, world!");
}
