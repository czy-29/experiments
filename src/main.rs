use std::thread;
use tokio::{runtime, time::{sleep, Duration}};

fn main() {
    let handle = thread::spawn(rt2);
    rt1();
    handle.join().unwrap();
}

#[tokio::main(flavor = "current_thread")]
async fn rt1() {
    run().await;
}

#[tokio::main]
async fn rt2() {
    run().await;
}

async fn run() {
    sleep_mini().await;
    print_flavor();
    sleep_mini().await;
}

async fn sleep_mini() {
    sleep(Duration::from_secs_f64(0.5)).await;
}

fn print_flavor() {
    println!("{:?}", runtime::Handle::current().runtime_flavor());
}
