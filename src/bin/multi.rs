use experiments::sleep2::sleep;
use std::thread;
use tokio::{
    runtime::{Handle, RuntimeFlavor},
    sync::OnceCell,
    task::JoinHandle,
    time::Duration,
};

static RT_C: OnceCell<Handle> = OnceCell::const_new();
static RT_M: OnceCell<Handle> = OnceCell::const_new();

fn main() {
    let handle = thread::spawn(rt_multi);
    rt_current();
    handle.join().unwrap();
}

#[tokio::main(flavor = "current_thread")]
async fn rt_current() {
    RT_C.set(Handle::current()).unwrap();
    run().await;
}

#[tokio::main]
async fn rt_multi() {
    RT_M.set(Handle::current()).unwrap();
    run().await;
}

async fn run() {
    sleep_mini().await;

    println!("{:?} spawn begin!", flavor());
    let h1 = spawn_current().await;
    let h2 = spawn_multi().await;
    println!("{:?} spawn end!", flavor());

    h1.await.unwrap();
    h2.await.unwrap();
    println!("{:?} joined!", flavor());

    sleep_mini().await;
    println!("{:?} finished!", flavor());
}

async fn sleep_mini() {
    sleep(Duration::from_secs_f64(1.5)).await;
}

fn flavor() -> RuntimeFlavor {
    Handle::current().runtime_flavor()
}

use rand::{rng, Rng};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::task::JoinError;

#[derive(Debug)]
pub struct SpawnHandle<T>(SpawnInner<T>);

impl<T> SpawnHandle<T> {
    pub fn wrap(output: T) -> Self {
        Self(output.into())
    }

    pub fn is_spawned(&self) -> bool {
        matches!(self.0, SpawnInner::Spawned(_))
    }

    pub fn is_ready(&self) -> bool {
        matches!(self.0, SpawnInner::Ready(_))
    }

    pub fn unwrap_join_handle(self) -> JoinHandle<T> {
        match self.0 {
            SpawnInner::Spawned(handle) => handle,
            SpawnInner::Ready(_) => panic!("SpawnHandle is not spawned"),
        }
    }

    pub fn unwrap_output(self) -> T {
        match self.0 {
            SpawnInner::Spawned(_) => panic!("SpawnHandle is not ready"),
            SpawnInner::Ready(output) => output.expect("output has been taken"),
        }
    }
}

impl<T: Unpin> Future for SpawnHandle<T> {
    type Output = Result<T, JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
    }
}

impl<T> From<JoinHandle<T>> for SpawnHandle<T> {
    fn from(handle: JoinHandle<T>) -> Self {
        Self(handle.into())
    }
}

impl<T> From<SpawnInner<T>> for SpawnHandle<T> {
    fn from(inner: SpawnInner<T>) -> Self {
        Self(inner)
    }
}

#[derive(Debug)]
enum SpawnInner<T> {
    Spawned(JoinHandle<T>),
    Ready(Option<T>),
}

impl<T: Unpin> Future for SpawnInner<T> {
    type Output = Result<T, JoinError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match &mut *self {
            SpawnInner::Spawned(handle) => Pin::new(handle).poll(cx),
            SpawnInner::Ready(o) => Poll::Ready(Ok(o.take().expect("polled after ready"))),
        }
    }
}

impl<T> From<JoinHandle<T>> for SpawnInner<T> {
    fn from(handle: JoinHandle<T>) -> Self {
        Self::Spawned(handle)
    }
}

impl<T> From<T> for SpawnInner<T> {
    fn from(output: T) -> Self {
        Self::Ready(Some(output))
    }
}

#[derive(Debug)]
pub struct SpawnFairness<F> {
    future: Option<Pin<Box<F>>>,
    poll_immediate: bool,
}

impl<F> SpawnFairness<F> {
    fn new(future: F, poll_immediate: bool) -> Self {
        Self {
            future: Some(Box::pin(future)),
            poll_immediate,
        }
    }

    fn spawn(future: F) -> Self {
        Self::new(future, false)
    }

    fn poll_spawn(future: F) -> Self {
        Self::new(future, true)
    }
}

impl<F> Future for SpawnFairness<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    type Output = SpawnHandle<F::Output>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut future = self.future.take().expect("polled twice");

        if self.poll_immediate {
            if let Poll::Ready(o) = Pin::new(&mut future).poll(cx) {
                return ready(o.into());
            }
        }

        ready(tokio::spawn(future).into())
    }
}

fn ready<T>(inner: SpawnInner<T>) -> Poll<SpawnHandle<T>> {
    Poll::Ready(inner.into())
}

pub fn spawn_immediate<F>(future: F) -> SpawnFairness<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    SpawnFairness::poll_spawn(future)
}

fn spawn_spawn<F>(future: F) -> SpawnFairness<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    SpawnFairness::spawn(future)
}

pub fn spawn_uniform<F>(future: F) -> SpawnFairness<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    spawn_uniform_with_rng(future, &mut rng())
}

pub fn spawn_fairness<F>(future: F, fairness: f64) -> SpawnFairness<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
{
    spawn_fairness_with_rng(future, fairness, &mut rng())
}

pub fn spawn_uniform_with_rng<F, R>(future: F, rng: &mut R) -> SpawnFairness<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
    R: Rng + ?Sized,
{
    SpawnFairness::new(future, rng.random())
}

pub fn spawn_fairness_with_rng<F, R>(future: F, fairness: f64, rng: &mut R) -> SpawnFairness<F>
where
    F: Future + Send + 'static,
    F::Output: Send + 'static,
    R: Rng + ?Sized,
{
    match fairness {
        1.0 => spawn_immediate(future),
        0.5 => spawn_uniform(future),
        0.0 => spawn_spawn(future),
        f if f < 0.0 || f > 1.0 => panic!("fairness must be in [0.0, 1.0]"),
        f => SpawnFairness::new(future, rng.random_bool(f)),
    }
}

async fn spawn_handle(handle: &Handle) -> SpawnHandle<()> {
    let task = async {
        println!("{:?} run once!", flavor());
        sleep(Duration::from_millis(200)).await;
        println!("{:?} run twice!", flavor());
    };

    if Handle::current().id() == handle.id() {
        spawn_fairness(task, 0.75).await
    } else {
        handle.spawn(task).into()
    }
}

async fn spawn_current() -> SpawnHandle<()> {
    spawn_handle(RT_C.get().unwrap()).await
}

async fn spawn_multi() -> SpawnHandle<()> {
    spawn_handle(RT_M.get().unwrap()).await
}
