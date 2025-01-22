use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    process::Termination,
    sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender},
    task::{Context, Poll},
    thread::{self, JoinHandle, LocalKey},
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
            Some(handles) => {
                if self.done {
                    panic!("cannot continue polling after sleep returns");
                } else {
                    match handles.elapsed.try_recv() {
                        Err(_) => Poll::Pending,
                        Ok(_) => {
                            self.done = true;
                            Poll::Ready(())
                        }
                    }
                }
            }
            None => {
                let (notify, elapsed) = channel();
                let (sleep_cancel, sleep_wait) = channel();
                let dur = self.dur;
                let waker = cx.waker().clone();
                let thread = thread::spawn(move || match sleep_wait.recv_timeout(dur) {
                    Ok(_) => unreachable!(),
                    Err(RecvTimeoutError::Disconnected) => (),
                    Err(RecvTimeoutError::Timeout) => {
                        if notify.send(()).is_ok() {
                            waker.wake();
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
    Sleep {
        dur,
        handles: None,
        done: false,
    }
}

// task, spawn(poll_immediate: Option<bool>), runtime, waker
// rand, with_cancel_signal(tokio::signal::ctrlc)

thread_local! {
    static RT: RefCell<Option<Runtime>> = Default::default();
}

#[derive(Debug, Clone)]
pub struct Runtime;

impl Runtime {
    pub fn current() -> Option<Self> {
        RT.cloned()
    }
}

#[derive(Debug)]
pub struct SetGuard<T: 'static> {
    key: &'static LocalKey<RefCell<T>>,
    old: Option<T>,
}

impl<T: 'static> SetGuard<T> {
    fn new(key: &'static LocalKey<RefCell<T>>, value: T) -> Self {
        let old = Some(key.replace(value));
        Self { key, old }
    }
}

impl<T: 'static + Clone> SetGuard<T> {
    pub fn cloned(&self) -> T {
        self.key.cloned()
    }
}

impl<T: 'static> Drop for SetGuard<T> {
    fn drop(&mut self) {
        self.key.replace(self.old.take().unwrap());
    }
}

pub trait LocalKeyExt<T: Clone> {
    fn cloned(&'static self) -> T;
}

impl<T: Clone> LocalKeyExt<T> for LocalKey<RefCell<T>> {
    fn cloned(&'static self) -> T {
        self.with_borrow(Clone::clone)
    }
}

pub trait ScopedSet<T> {
    #[must_use]
    fn scoped_set(&'static self, value: T) -> SetGuard<T>;

    #[must_use]
    fn scoped_set_if<P>(&'static self, value: T, predicate: P) -> Option<SetGuard<T>>
    where
        P: FnOnce(&T) -> bool;
}

impl<T> ScopedSet<T> for LocalKey<RefCell<T>> {
    fn scoped_set(&'static self, value: T) -> SetGuard<T> {
        SetGuard::new(self, value)
    }

    fn scoped_set_if<P>(&'static self, value: T, predicate: P) -> Option<SetGuard<T>>
    where
        P: FnOnce(&T) -> bool,
    {
        if self.with_borrow(predicate) {
            Some(self.scoped_set(value))
        } else {
            None
        }
    }
}

pub trait ScopedSetSome<T> {
    #[must_use]
    fn scoped_set_some(&'static self, value: T) -> Option<SetGuard<Option<T>>>;
}

impl<T> ScopedSetSome<T> for LocalKey<RefCell<Option<T>>> {
    fn scoped_set_some(&'static self, value: T) -> Option<SetGuard<Option<T>>> {
        self.scoped_set_if(Some(value), Option::is_none)
    }
}

fn main() -> impl Termination {
    println!("Hello, world!");
}

async fn async_main() -> impl Termination {
    println!("Hello, world!");
}
