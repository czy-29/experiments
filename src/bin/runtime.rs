#![forbid(unsafe_code)]

use std::{
    any::Any,
    cell::RefCell,
    collections::BTreeMap,
    fmt::{self, Debug, Formatter},
    future::Future,
    pin::Pin,
    process::Termination,
    rc::Rc,
    sync::{
        mpsc::{channel, IntoIter, Receiver, RecvError, RecvTimeoutError, Sender},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
    thread::{self, JoinHandle, LocalKey},
    time::Duration,
};

use rand::random;
use tokio::signal::ctrl_c;

#[derive(Debug)]
pub struct WithCancelSignal<F: Future, C: Future> {
    future: Pin<Box<F>>,
    cancel: Pin<Box<C>>,
}

impl<F, C> Future for WithCancelSignal<F, C>
where
    F: Future,
    C: Future,
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

pub trait FutureExt: Future + Sized {
    fn with_cancel_signal<C: Future>(self, cancel: C) -> WithCancelSignal<Self, C> {
        WithCancelSignal {
            future: Box::pin(self),
            cancel: Box::pin(cancel),
        }
    }
}

impl<T: Future + Sized> FutureExt for T {}

pub type BoxAnyLocal = Box<dyn Any>;
pub type BoxAny = Box<dyn Any + Send>;

pub type AnyFutureLocal<O> = Pin<Box<dyn Future<Output = O>>>;
pub type AnyFuture<O> = Pin<Box<dyn Future<Output = O> + Send>>;

pub type AnyFutureLocalAnyLocal = AnyFutureLocal<BoxAnyLocal>;
pub type AnyFutureLocalAny = AnyFutureLocal<BoxAny>;
pub type AnyFutreAnyLocal = AnyFuture<BoxAnyLocal>; // 这东西不一定真的存在
pub type AnyFutureAny = AnyFuture<BoxAny>;

pub fn any_local<T: 'static>(value: T) -> BoxAnyLocal {
    Box::new(value) as _
}

pub fn any_send<T: Send + 'static>(value: T) -> BoxAny {
    Box::new(value) as _
}

pub fn force_downcast_local<T: 'static>(local: BoxAnyLocal) -> T {
    *local.downcast().unwrap()
}

pub fn force_downcast_any<T: 'static>(any: BoxAny) -> T {
    *any.downcast().unwrap()
}

pub fn any_future_local_any_local<T>(future: T) -> AnyFutureLocalAnyLocal
where
    T: Future + 'static,
{
    Box::pin(async move { any_local(future.await) })
}

pub fn any_future_local_any<T>(future: T) -> AnyFutureLocalAny
where
    T: Future + 'static,
    T::Output: Send,
{
    Box::pin(async move { any_send(future.await) })
}

pub fn any_future_any_local<T>(future: T) -> AnyFutreAnyLocal
where
    T: Future + Send + 'static,
{
    Box::pin(async move { any_local(future.await) })
}

pub fn any_future_any<T>(future: T) -> AnyFutureAny
where
    T: Future + Send + 'static,
    T::Output: Send,
{
    Box::pin(async move { any_send(future.await) })
}

use self::any_future_local_any as rt_future;
use self::force_downcast_any as downcast_rt;
use self::AnyFutureLocalAny as RtFuture;
use self::BoxAny as RtOutput;

#[derive(Debug)]
struct FutureHandles<T> {
    thread: JoinHandle<()>,
    output: Receiver<T>,
    cancelable_sender: RtSender,
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

#[derive(Debug)]
pub struct Task<T: Send + 'static> {
    id: u64,
    handles: Option<FutureHandles<T>>,
    done: bool,
    output_channel: Option<(RtSender, RtWaiter)>,
}

impl<T: Send + 'static> Task<T> {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn is_finished(&self) -> bool {
        self.done
            || match &self.handles {
                None => true,
                Some(handles) => handles.thread.is_finished(),
            }
    }
}

impl<T: Send + 'static> Drop for Task<T> {
    fn drop(&mut self) {
        if let Some(handles) = self.handles.take() {
            drop(handles.output);
            handles.cancelable_sender.cancel();
            drop(handles.cancelable_sender);
            handles.thread.join().ok();
        }
    }
}

impl<T: Send + 'static> Future for Task<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &self.handles {
            Some(handles) => {
                if self.done {
                    panic!("cannot continue polling after future returns");
                } else {
                    match handles.output.try_recv() {
                        Err(_) => Poll::Pending,
                        Ok(output) => {
                            self.done = true;
                            Poll::Ready(output)
                        }
                    }
                }
            }
            None => {
                let (notify, output) = channel();
                let (cancelable_sender, cancelable_waiter) = self.output_channel.take().unwrap();
                let waker = cx.waker().clone();
                let thread = thread::spawn(move || match cancelable_waiter.wait_output() {
                    WaitResult::Elapsed => unreachable!(),
                    WaitResult::Canceled => (),
                    WaitResult::Output(output) => {
                        if notify.send(downcast_rt(output)).is_ok() {
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

fn task<T: Send + 'static>(task_id: u64) -> (Task<T>, RtSender) {
    let rt_channel = rt_channel();
    let rt_sender = rt_channel.0.clone();
    let task = Task {
        id: task_id,
        handles: None,
        done: false,
        output_channel: Some(rt_channel),
    };

    (task, rt_sender)
}

thread_local! {
    static RT: RefCell<Option<Runtime>> = Default::default();
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Hash)]
enum Event {
    Spawn,
    Wake,
}

#[derive(Debug, Clone)]
struct EventSender(Sender<(Event, u64)>);

impl EventSender {
    fn spawn(&self, task_id: u64) {
        self.send(Event::Spawn, task_id);
    }

    fn wake(&self, task_id: u64) {
        self.send(Event::Wake, task_id);
    }

    fn send(&self, event: Event, task_id: u64) {
        self.0.send((event, task_id)).ok();
    }
}

#[derive(Debug)]
struct EventReceiver(Receiver<(Event, u64)>);

impl IntoIterator for EventReceiver {
    type Item = (Event, u64);
    type IntoIter = IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

fn event_channel() -> (EventSender, EventReceiver) {
    let (send, recv) = channel();
    (EventSender(send), EventReceiver(recv))
}

#[derive(Debug, Clone)]
struct RtSender(Sender<Result<RtOutput, ()>>);

impl RtSender {
    fn send_output(&self, output: RtOutput) {
        self.0.send(Ok(output)).ok();
    }

    fn cancel(&self) {
        self.0.send(Err(())).ok();
    }
}

#[derive(Debug)]
enum WaitResult {
    Output(RtOutput),
    Canceled,
    Elapsed,
}

#[derive(Debug)]
struct RtWaiter(Receiver<Result<RtOutput, ()>>);

impl RtWaiter {
    fn wait_for(self, dur: Duration) -> WaitResult {
        match self.0.recv_timeout(dur) {
            Ok(Ok(output)) => WaitResult::Output(output),
            Ok(Err(())) | Err(RecvTimeoutError::Disconnected) => WaitResult::Canceled,
            Err(RecvTimeoutError::Timeout) => WaitResult::Elapsed,
        }
    }

    fn wait_output(self) -> WaitResult {
        match self.0.recv() {
            Ok(Ok(output)) => WaitResult::Output(output),
            Ok(Err(())) | Err(RecvError) => WaitResult::Canceled,
        }
    }
}

fn rt_channel() -> (RtSender, RtWaiter) {
    let (send, recv) = channel();
    (RtSender(send), RtWaiter(recv))
}

#[derive(Debug, Clone)]
struct TaskWaker {
    sender: EventSender,
    task_id: u64,
}

impl Wake for TaskWaker {
    fn wake(self: Arc<Self>) {
        self.sender.wake(self.task_id);
    }
}

impl From<TaskWaker> for Waker {
    fn from(value: TaskWaker) -> Self {
        Arc::new(value).into()
    }
}

struct TaskContext {
    id: u64,
    waker: Waker,
    sender: RtSender,
    future: RtFuture,
}

impl TaskContext {
    fn poll(&mut self) -> Poll<RtOutput> {
        self.future
            .as_mut()
            .poll(&mut Context::from_waker(&self.waker))
    }

    fn send_output(&self, output: RtOutput) {
        self.sender.send_output(output);
    }

    fn handle_entry<T: 'static>(&self, output: RtOutput) -> Option<T> {
        if self.id == 0 {
            Some(downcast_rt(output))
        } else {
            self.send_output(output);
            None
        }
    }
}

impl Debug for TaskContext {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("TaskContext")
            .field("id", &self.id)
            .field("waker", &self.waker)
            .field("sender", &self.sender)
            .field("future", &"RtFuture")
            .finish()
    }
}

#[derive(Debug, Clone, Default)]
struct TaskMap(Rc<RefCell<BTreeMap<u64, TaskContext>>>);

impl TaskMap {
    fn push_task(&self, task_context: TaskContext) {
        self.0.borrow_mut().insert(task_context.id, task_context);
    }

    fn remove_task(&self, task_id: u64) -> TaskContext {
        self.0.borrow_mut().remove(&task_id).unwrap()
    }

    fn exists(&self, task_id: u64) -> bool {
        self.0.borrow().contains_key(&task_id)
    }
}

#[derive(Debug, Clone, Default)]
struct IdGen(Rc<RefCell<u64>>);

impl IdGen {
    fn next(&self) -> u64 {
        let next_id = *self.0.borrow();
        *self.0.borrow_mut() += 1;
        next_id
    }
}

#[derive(Debug, Clone)]
pub struct Runtime {
    id_gen: IdGen,
    event_sender: EventSender,
    task_map: TaskMap,
}

impl Runtime {
    pub fn try_current() -> Option<Self> {
        RT.cloned()
    }

    pub fn current() -> Self {
        Self::try_current().expect("runtime has not been started")
    }

    pub fn main<T>(spin_on_wake: bool, entry_future: T) -> T::Output
    where
        T: Future + 'static,
        T::Output: Termination + Send,
    {
        let (rt, event_recver) = Self::new();
        let _guard = RT
            .scoped_set_some(rt)
            .expect("cannot create another runtime within a runtime");

        {
            let rt = Self::current();
            let (task_context, _) = rt.new_task(entry_future);
            rt.push_task(task_context, true);
        }

        for (event, task_id) in event_recver {
            let mut task_context = {
                let task_map = &Self::current().task_map;

                if !task_map.exists(task_id) {
                    continue;
                }

                task_map.remove_task(task_id)
            };

            match task_context.poll() {
                Poll::Ready(output) => {
                    if let Some(output) = task_context.handle_entry(output) {
                        return output;
                    }
                }
                Poll::Pending => match event {
                    Event::Spawn => {
                        Self::current().task_map.push_task(task_context);
                    }
                    Event::Wake => {
                        if spin_on_wake {
                            let output = loop {
                                if let Poll::Ready(output) = task_context.poll() {
                                    break output;
                                }
                            };

                            if let Some(output) = task_context.handle_entry(output) {
                                return output;
                            }
                        } else {
                            let rt = Self::current();
                            rt.task_map.push_task(task_context);
                            rt.event_sender.wake(task_id);
                        }
                    }
                },
            }
        }

        unreachable!()
    }

    fn new() -> (Self, EventReceiver) {
        let (event_sender, event_recver) = event_channel();
        let runtime = Self {
            id_gen: Default::default(),
            event_sender,
            task_map: Default::default(),
        };

        (runtime, event_recver)
    }

    fn waker(&self, task_id: u64) -> Waker {
        let sender = self.event_sender.clone();
        TaskWaker { sender, task_id }.into()
    }

    fn new_task<T>(&self, future: T) -> (TaskContext, Task<T::Output>)
    where
        T: Future + 'static,
        T::Output: Send,
    {
        let id = self.id_gen.next();
        let waker = self.waker(id);
        let (task, sender) = task(id);
        let future = rt_future(future);
        let task_context = TaskContext {
            id,
            waker,
            sender,
            future,
        };

        (task_context, task)
    }

    fn push_task(&self, task_context: TaskContext, spawn: bool) {
        let task_id = task_context.id;

        self.task_map.push_task(task_context);

        if spawn {
            self.event_sender.spawn(task_id);
        }
    }
}

#[derive(Debug)]
#[must_use]
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
    fn scoped_set(&'static self, value: T) -> SetGuard<T>;

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
    fn scoped_set_some(&'static self, value: T) -> Option<SetGuard<Option<T>>>;
}

impl<T> ScopedSetSome<T> for LocalKey<RefCell<Option<T>>> {
    fn scoped_set_some(&'static self, value: T) -> Option<SetGuard<Option<T>>> {
        self.scoped_set_if(Some(value), Option::is_none)
    }
}

pub fn spawn<T>(poll_immediate: Option<bool>, future: T) -> Task<T::Output>
where
    T: Future + 'static,
    T::Output: Send,
{
    let rt = Runtime::current();
    let (mut task_context, task) = rt.new_task(future);

    if poll_immediate.unwrap_or_else(random) {
        match task_context.poll() {
            Poll::Ready(output) => {
                task_context.send_output(output);
            }
            Poll::Pending => {
                rt.push_task(task_context, false);
            }
        }
    } else {
        rt.push_task(task_context, true);
    }

    task
}

fn main() -> impl Termination {
    Runtime::main(true, async_main())
}

async fn async_main() -> impl Termination {
    println!("start");

    let dur = Duration::from_secs_f64(3.0);
    let t1 = spawn(None, sleep(dur));
    let t2 = spawn(None, async move { sleep(dur).await });
    let join = async move {
        t1.await;
        t2.await;
    };

    join.with_cancel_signal(ctrl_c())
        .await
        .inspect(|_| println!("finished"))
        .inspect_err(|_| println!("ctrl-c"))
        .ok();
}
