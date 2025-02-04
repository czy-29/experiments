use std::{
    collections::BTreeSet,
    env::args,
    process::exit,
    time::{Duration, Instant},
};
use tokio::{
    net::{lookup_host, TcpStream},
    task::JoinSet,
};
use tokio_util::time::FutureExt;

#[tokio::main]
async fn main() {
    let now = Instant::now();
    let host = args().nth(1).unwrap_or("127.0.0.1".into());

    match lookup_host(format!("{}:0", host))
        .timeout(Duration::from_secs(3))
        .await
    {
        Ok(Ok(_)) => {}
        _ => {
            eprintln!("cannot resolve host: {}", host);
            exit(1);
        }
    }

    println!("scanning host: {}", host);
    let mut ports = BTreeSet::new();

    for iters in 0..32 {
        let start = iters * 2048;

        println!(
            "scanning port: {}-{} ({}/32)",
            if start == 0 { 1 } else { start },
            start + 2047,
            iters + 1
        );

        let mut scans = JoinSet::new();

        for index in 0..2048 {
            let port = start + index;

            if port == 0 {
                continue;
            }

            let host = host.clone();
            scans.spawn(async move {
                match TcpStream::connect((host, port))
                    .timeout(Duration::from_secs(10))
                    .await
                {
                    Ok(Ok(_)) => {
                        println!("found: {}", port);
                        Ok(port)
                    }
                    _ => Err(()),
                }
            });
        }

        for port in scans.join_all().await.into_iter().filter_map(Result::ok) {
            ports.insert(port);
        }
    }

    println!();
    println!("full scan complete!");
    println!("ports found for host {}:", host);
    for port in ports {
        println!("{}", port);
    }
    println!("scan took: {:?}", now.elapsed());
}
