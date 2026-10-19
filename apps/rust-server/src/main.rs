use std::{fs::OpenOptions, path::PathBuf};
use storyfold_server::{AppError, Result, Store};

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Storyfold: {e}");
        std::process::exit(1);
    }
}
async fn run() -> Result<()> {
    let mut root = std::env::var_os("STORYFOLD_ROOT")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let mut data = None;
    let mut port = None;
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let value = args
            .next()
            .ok_or_else(|| AppError::bad("Expected a value after --root, --data or --port."))?;
        match flag.as_str() {
            "--root" => root = PathBuf::from(value),
            "--data" => data = Some(PathBuf::from(value)),
            "--port" => port = Some(value),
            _ => {
                return Err(AppError::bad(
                    "Supported arguments: --root, --data, --port.",
                ));
            }
        }
    }
    let root = root.canonicalize()?;
    let _ = dotenvy::from_path(root.join(".env"));
    let data = data
        .or_else(|| {
            std::env::var_os("STORYFOLD_DATA_DIR")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| root.join("data"));
    let data = if data.is_absolute() {
        data
    } else {
        root.join(data)
    };
    let store = Store::new(&root, &data)?;
    let lock_path = store.data.join("rust-server.lock");
    storyfold_server::storage::reject_link(&lock_path)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&lock_path)?;
    fs2::FileExt::try_lock_exclusive(&lock).map_err(|_|AppError::bad("Another Rust server is using this project data. Reuse that server or choose a different data folder."))?;
    let port: u16 = port
        .or_else(|| std::env::var("STORYFOLD_PORT").ok())
        .unwrap_or_else(|| "4311".into())
        .parse()
        .map_err(|_| AppError::bad("Invalid local server port."))?;
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    println!(
        "Storyfold Rust server: http://127.0.0.1:{}",
        listener.local_addr()?.port()
    );
    axum::serve(listener, storyfold_server::api::router(store))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    drop(lock);
    Ok(())
}
