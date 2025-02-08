use std::{
    any::Any,
    future::Future,
    pin::Pin,
    sync::mpsc::{channel, Receiver, RecvError, RecvTimeoutError, Sender, TryRecvError},
    task::{Context, Poll},
    thread::{self, JoinHandle},
    time::Duration,
};

pub type RtOutput = Box<dyn Any + Send>;

#[derive(Debug)]
pub struct FutureHandles<T> {
    pub thread: JoinHandle<()>,
    pub output: Receiver<T>,
    pub cancelable_sender: RtSender,
}

#[derive(Debug)]
pub struct Sleep {
    dur: Duration,
    handles: Option<FutureHandles<()>>,
    done: bool,
}

impl Sleep {
    pub fn dur(&self) -> Duration {
        self.dur
    }

    pub fn is_elapsed(&self) -> bool {
        self.done
            || match &self.handles {
                None => true,
                Some(handles) => handles.thread.is_finished(),
            }
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        if let Some(handles) = self.handles.take() {
            drop(handles.output);
            handles.cancelable_sender.cancel();
            drop(handles.cancelable_sender);
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
                    panic!("cannot continue polling after future returns");
                } else {
                    match handles.output.try_recv() {
                        Err(_) => Poll::Pending,
                        Ok(_) => {
                            self.done = true;
                            Poll::Ready(())
                        }
                    }
                }
            }
            None => {
                let (notify, output) = channel();
                let (cancelable_sender, cancelable_waiter) = rt_channel();
                let dur = self.dur;
                let waker = cx.waker().clone();
                let thread = thread::spawn(move || match cancelable_waiter.wait_for(dur) {
                    WaitResult::Output(_) => unreachable!(),
                    WaitResult::Canceled => (),
                    WaitResult::Elapsed => {
                        if notify.send(()).is_ok() {
                            waker.wake();
                        }
                    }
                });
                let handles = FutureHandles {
                    thread,
                    output,
                    cancelable_sender,
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

#[derive(Debug, Clone)]
pub struct RtSender(Sender<Result<RtOutput, ()>>);

impl RtSender {
    pub fn send_output(&self, output: RtOutput) {
        self.0.send(Ok(output)).ok();
    }

    pub fn cancel(&self) {
        self.0.send(Err(())).ok();
    }
}

#[derive(Debug)]
pub enum WaitResult {
    Output(RtOutput),
    Canceled,
    Elapsed,
}

#[derive(Debug)]
pub struct RtWaiter(Receiver<Result<RtOutput, ()>>);

impl RtWaiter {
    pub fn wait_for(self, dur: Duration) -> WaitResult {
        match self.0.recv_timeout(dur) {
            Ok(Ok(output)) => WaitResult::Output(output),
            Ok(Err(())) | Err(RecvTimeoutError::Disconnected) => WaitResult::Canceled,
            Err(RecvTimeoutError::Timeout) => WaitResult::Elapsed,
        }
    }

    pub fn wait_output(self) -> WaitResult {
        match self.0.recv() {
            Ok(Ok(output)) => WaitResult::Output(output),
            Ok(Err(())) | Err(RecvError) => WaitResult::Canceled,
        }
    }

    pub fn try_wait_output(&self) -> Option<WaitResult> {
        match self.0.try_recv() {
            Ok(Ok(output)) => Some(WaitResult::Output(output)),
            Ok(Err(())) | Err(TryRecvError::Disconnected) => Some(WaitResult::Canceled),
            Err(TryRecvError::Empty) => None,
        }
    }
}

pub fn rt_channel() -> (RtSender, RtWaiter) {
    let (send, recv) = channel();
    (RtSender(send), RtWaiter(recv))
}
