#[tokio::main(flavor = "current_thread")]
async fn main() {
    for _ in 0..100 {
        inout().await;
        println!();
    }
}

async fn inout() {
    let inner = tokio::spawn(async move {
        println!("inner");
    });
    println!("outer");
    inner.await.unwrap();
}
