use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};

fn main() {
    #[cfg(target_os = "linux")]
    if let Err(error) = host_gtk_environment() {
        eprintln!("Storyfold: {error}");
        std::process::exit(1);
    }
    match prepare_backend() {
        Ok(child) => storyfold_desktop::launch_with_backend(child),
        Err(error) => {
            eprintln!("Storyfold: {error}");
            rfd::MessageDialog::new()
                .set_title("Storyfold could not start")
                .set_description(&error)
                .set_level(rfd::MessageLevel::Error)
                .show();
            std::process::exit(1);
        }
    }
}

#[cfg(target_os = "linux")]
fn host_gtk_environment() -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    // A Snap-hosted IDE exports loader paths for its older bundled glibc. System
    // WebKit must use the host GTK/GIO loaders. Re-exec before creating any threads;
    // only confirmed Snap loader overrides are removed, never security settings.
    let keys = [
        "GIO_MODULE_DIR",
        "GTK_PATH",
        "GDK_PIXBUF_MODULE_FILE",
        "GDK_PIXBUF_MODULEDIR",
    ];
    let incompatible: Vec<_> = keys
        .into_iter()
        .filter(|key| {
            std::env::var_os(key).is_some_and(|value| value.to_string_lossy().contains("/snap/"))
        })
        .collect();
    if incompatible.is_empty() {
        return Ok(());
    }
    let executable = std::env::current_exe()
        .map_err(|_| "Could not locate Storyfold for its host GTK launch.")?;
    let mut command = Command::new(executable);
    command.args(std::env::args_os().skip(1));
    for key in incompatible {
        command.env_remove(key);
    }
    Err(format!(
        "Could not launch with the host GTK libraries: {}",
        command.exec()
    ))
}

fn prepare_backend() -> Result<Option<Child>, String> {
    let base = std::env::var("STORYFOLD_SERVER").unwrap_or_else(|_| "http://127.0.0.1:4311".into());
    let api = storyfold_desktop::api::Api::new(&base)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "Could not start the local service connection.")?;
    let healthy = |runtime: &tokio::runtime::Runtime| {
        runtime.block_on(async {
            tokio::time::timeout(
                Duration::from_secs(2),
                api.get::<serde_json::Value>("/health"),
            )
            .await
            .ok()
            .and_then(Result::ok)
            .is_some_and(|v| v["ok"] == true && v["runtime"] == "Rust")
        })
    };
    if healthy(&runtime) {
        return Ok(None);
    }
    if std::env::var_os("STORYFOLD_SERVER").is_some() {
        return Err("The configured Rust service is unavailable. A custom STORYFOLD_SERVER address must point to an already running local service.".into());
    }
    let executable =
        std::env::current_exe().map_err(|_| "Could not locate the desktop application.")?;
    let server = executable.with_file_name(if cfg!(windows) {
        "storyfold-server.exe"
    } else {
        "storyfold-server"
    });
    if !server.is_file() {
        return Err("The storyfold-server executable is missing beside this application. Build or install the complete Storyfold package.".into());
    }
    let root = std::env::var_os("STORYFOLD_ROOT")
        .map(PathBuf::from)
        .or_else(|| {
            executable
                .ancestors()
                .find(|p| {
                    p.join("apps/rust-server").is_dir()
                        || p.join("scripts/whisper.py").is_file()
                            && p.join(".catalyst/pipeline.json").is_file()
                })
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."));
    let root = root.canonicalize().map_err(
        |_| "The Storyfold project folder is unavailable. Set STORYFOLD_ROOT to its location.",
    )?;
    // The service reads its private environment itself. Keys never enter the WebView or startup logs.
    let mut child = Command::new(server)
        .arg("--root")
        .arg(&root)
        .arg("--port")
        .arg("4311")
        .current_dir(root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| "Could not start the bundled Rust service.")?;
    let started = std::time::Instant::now();
    while started.elapsed() < Duration::from_secs(20) {
        if child
            .try_wait()
            .map_err(|_| "Could not check the local service.")?
            .is_some()
        {
            return Err("The Rust service stopped during startup. Check that the data folder is writable and port 4311 is available.".into());
        }
        if healthy(&runtime) {
            return Ok(Some(child));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    storyfold_desktop::stop_owned_backend(&mut child);
    Err("The Rust service did not become ready. Check the local media tools and data folder, then retry.".into())
}
