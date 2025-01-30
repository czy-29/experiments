use std::thread;
use tokio::{
    runtime::{Handle, RuntimeFlavor},
    sync::OnceCell,
    task::JoinHandle,
    time::{sleep, Duration},
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
    let h1 = spawn_current();
    let h2 = spawn_multi();
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

fn spawn_handle(handle: &Handle) -> JoinHandle<()> {
    let task = async {
        println!("{:?} run once!", flavor());
        sleep(Duration::from_nanos(1)).await;
        println!("{:?} run twice!", flavor());
    };

    if Handle::current().id() == handle.id() {
        tokio::spawn(task)
    } else {
        handle.spawn(task)
    }
}

fn spawn_current() -> JoinHandle<()> {
    spawn_handle(RT_C.get().unwrap())
}

fn spawn_multi() -> JoinHandle<()> {
    spawn_handle(RT_M.get().unwrap())
}
