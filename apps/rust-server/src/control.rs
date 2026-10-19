use crate::{
    AppError, Result, Store,
    api::AppState,
    storage::{read_json, reject_link, write_json},
};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    routing::{get, patch, post},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    path::{Component, Path as FsPath, PathBuf},
    process::Stdio,
    sync::Arc,
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
pub struct Control {
    store: Store,
    lock: Arc<Mutex<()>>,
    run: Arc<Mutex<Option<Value>>>,
    cancel: Arc<Mutex<Option<CancellationToken>>>,
}
impl Control {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            lock: Default::default(),
            run: Default::default(),
            cancel: Default::default(),
        }
    }
}
fn hash(s: &str) -> String {
    format!("{:x}", Sha256::digest(s.as_bytes()))
}
pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/catalyst", get(dashboard))
        .route("/api/catalyst/files", get(files))
        .route("/api/catalyst/file", get(source))
        .route("/api/catalyst/site", get(site))
        .route("/api/catalyst/roadmap", get(roadmap))
        .route("/api/catalyst/control", post(set_control))
        .route("/api/catalyst/tasks", post(add_task))
        .route("/api/catalyst/tasks/{id}", patch(change_task))
        .route("/api/catalyst/checks", post(start_check))
        .route("/api/catalyst/checks/cancel", post(cancel_check))
        .route("/api/catalyst/proposals", post(propose))
        .route("/api/catalyst/proposals/{id}/apply", post(apply))
        .route("/api/catalyst/proposals/{id}/discard", post(discard))
        .route("/api/catalyst/proposals/{id}/undo", post(undo))
}
fn private(c: &Control, sub: &str) -> Result<PathBuf> {
    let p = c.store.root.join(".catalyst/private").join(sub);
    c.store.directory(&p)?;
    Ok(p)
}
fn safe_file(c: &Control, relative: &str) -> Result<PathBuf> {
    let parts: Vec<_> = relative.split('/').collect();
    let root = parts.first().copied().unwrap_or("");
    let fixed = [
        "README.md",
        "BUILD_PROMPT.md",
        "Cargo.toml",
        "package.json",
        "tsconfig.json",
        "eslint.config.js",
        ".catalyst/pipeline.json",
        ".catalyst/instructions.md",
    ];
    if !["apps", "packages", "docs", "scripts", "tests"].contains(&root)
        && !fixed.contains(&relative)
    {
        return Err(AppError::bad(
            "Catalyst only edits this project's source, documentation and build instructions.",
        ));
    }
    if relative.contains('\\')
        || parts.iter().any(|p| {
            p.is_empty()
                || *p == ".."
                || *p == "."
                || p.starts_with(".env")
                || p.starts_with(".git")
                || ["node_modules", "target", "dist", "data"].contains(p)
                || p.to_ascii_lowercase().contains("secret")
                || p.to_ascii_lowercase().contains("credential")
        })
    {
        return Err(AppError::bad("That source path is private or unavailable."));
    }
    if ![
        "rs", "toml", "py", "sh", "ts", "tsx", "js", "jsx", "mjs", "css", "html", "json", "md",
        "svg", "yml", "yaml",
    ]
    .contains(
        &FsPath::new(relative)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or(""),
    ) {
        return Err(AppError::bad(
            "Only non-private text source files can be opened.",
        ));
    }
    let mut p = c.store.root.clone();
    for part in FsPath::new(relative).components() {
        let Component::Normal(part) = part else {
            return Err(AppError::bad("Invalid source path."));
        };
        p.push(part);
        reject_link(&p)?;
    }
    if !p.is_file() || std::fs::metadata(&p)?.len() > 512000 {
        return Err(AppError::bad("Source file is missing or exceeds 512 kB."));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if std::fs::metadata(&p)?.nlink() > 1 {
            return Err(AppError::bad("Linked source files are not editable."));
        }
    }
    Ok(p)
}
fn secret_scan(text: &str) -> Result<()> {
    let re=regex::Regex::new(concat!(r"-----BEGIN (?:[A-Z ]+ )?PRIVATE KEY-----|",r"sk-ant-[A-Za-z0-9_-]{12,}|sk-(?:proj-)?[A-Za-z0-9_-]{24,}|",r"gh[pousr]_[A-Za-z0-9]{20,}|AKIA[A-Z0-9]{16}|",r#"(?i:(?:api[_-]?key|secret|password|access[_-]?token)\s*[=:]\s*["']?[A-Za-z0-9_+/=-]{20,})"#)).expect("fixed regex");
    if re.is_match(text) {
        return Err(AppError::bad(
            "Possible credential found. Keep credentials in your private environment.",
        ));
    }
    Ok(())
}
fn list_files(c: &Control) -> Result<Vec<String>> {
    fn walk(c: &Control, p: &FsPath, out: &mut Vec<String>) -> Result<()> {
        if !p.exists() {
            return Ok(());
        }
        for entry in std::fs::read_dir(p)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let kind = entry.file_type()?;
            if kind.is_symlink()
                || name.starts_with('.')
                || ["node_modules", "target", "dist", "data", "public"].contains(&name.as_ref())
            {
                continue;
            }
            if kind.is_dir() {
                walk(c, &entry.path(), out)?;
            } else if let Ok(relative) = entry.path().strip_prefix(&c.store.root) {
                let s = relative.to_string_lossy().replace('\\', "/");
                if safe_file(c, &s).is_ok() {
                    out.push(s);
                }
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    for p in ["apps", "packages", "docs", "scripts", "tests"] {
        walk(c, &c.store.root.join(p), &mut files)?;
    }
    for p in [
        "README.md",
        "BUILD_PROMPT.md",
        "Cargo.toml",
        "package.json",
        ".catalyst/pipeline.json",
        ".catalyst/instructions.md",
    ] {
        if safe_file(c, p).is_ok() {
            files.push(p.into());
        }
    }
    files.sort();
    Ok(files)
}
async fn pipeline(c: &Control) -> Result<Value> {
    read_json(&c.store.root.join(".catalyst/pipeline.json")).await
}
async fn paused(c: &Control) -> Result<Value> {
    let p = c.store.root.join(".catalyst/control.json");
    reject_link(&p)?;
    if p.exists() {
        read_json(&p).await
    } else {
        Ok(json!({"paused":false}))
    }
}
fn proposal_path(c: &Control, id: &str) -> Result<PathBuf> {
    if !storyfold_core::identifier(id, 60) {
        return Err(AppError::bad("Invalid proposal."));
    }
    let p = private(c, "proposals")?.join(format!("{id}.json"));
    reject_link(&p)?;
    Ok(p)
}
async fn proposals(c: &Control) -> Result<Vec<Value>> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(private(c, "proposals")?)? {
        let e = e?;
        if e.file_type()?.is_symlink() {
            return Err(AppError::bad("Linked history is not supported."));
        }
        if e.path().extension().is_some_and(|x| x == "json") {
            let v: Value = read_json(&e.path()).await?;
            secret_scan(&v.to_string())?;
            out.push(v);
        }
    }
    out.sort_by(|a, b| b["createdAt"].as_str().cmp(&a["createdAt"].as_str()));
    Ok(out)
}
async fn git(c: &Control, args: &[&str], index: Option<&FsPath>) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.current_dir(&c.store.root)
        .args([
            "--literal-pathspecs",
            "-c",
            "core.hooksPath=/dev/null",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.attributesFile=/dev/null",
        ])
        .args(args)
        .env("GIT_ATTR_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0");
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index);
    }
    let out = cmd.output().await?;
    if !out.status.success() {
        return Err(AppError::bad(format!(
            "Git {} failed. Check repository access and local Git identity.",
            args.first().unwrap_or(&"operation")
        )));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().into())
}
async fn history(c: &Control) -> Result<Value> {
    let log = git(c, &["log", "-20", "--format=%h%x09%aI%x09%s"], None).await?;
    let commits: Vec<_> = log
        .lines()
        .map(|line| {
            let mut p = line.splitn(3, '\t');
            json!({"id":p.next(),"at":p.next(),"message":p.next()})
        })
        .collect();
    Ok(json!({"commits":commits,"status":git(c,&["status","--short"],None).await?}))
}
async fn recent_runs(c: &Control) -> Result<Vec<Value>> {
    let dir = c.store.root.join(".catalyst/runs");
    c.store.directory(&dir)?;
    let mut values = Vec::new();
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        if e.file_type()?.is_file()
            && e.path().extension().is_some_and(|x| x == "json")
            && let Ok(mut v) = read_json::<Value>(&e.path()).await
        {
            if v["status"] == "running"
                && c.run
                    .lock()
                    .await
                    .as_ref()
                    .is_none_or(|active| active["id"] != v["id"])
            {
                v["status"] = json!("cancelled");
            }
            values.push(v);
        }
    }
    values.sort_by(|a, b| b["startedAt"].as_str().cmp(&a["startedAt"].as_str()));
    values.truncate(10);
    Ok(values)
}
async fn dashboard(State(s): State<AppState>) -> Result<Json<Value>> {
    let c = &s.control;
    let run = c.run.lock().await.clone();
    let runs = recent_runs(c).await?;
    Ok(Json(
        json!({"pipeline":pipeline(c).await?,"control":paused(c).await?,"run":run,"runs":runs,"tools":crate::media::tools_status(&s.store).await,"history":history(c).await?,"proposals":proposals(c).await?}),
    ))
}
async fn files(State(s): State<AppState>) -> Result<Json<Vec<String>>> {
    Ok(Json(list_files(&s.control)?))
}
async fn source(
    State(s): State<AppState>,
    Query(q): Query<HashMap<String, String>>,
) -> Result<Json<Value>> {
    let path = q
        .get("path")
        .ok_or_else(|| AppError::bad("Choose a source file."))?;
    let content = std::fs::read_to_string(safe_file(&s.control, path)?)?;
    secret_scan(&content)?;
    Ok(Json(
        json!({"path":path,"content":content,"hash":hash(&content)}),
    ))
}
async fn site(State(s): State<AppState>) -> Result<Json<Value>> {
    Ok(Json(
        json!({"name":"Storyfold","domain":"localhost","stack":"Rust / Dioxus / Axum","pages":[{"id":"editor","name":"Storyfold editor","route":"/","file":"apps/rust-desktop/src/editor.rs","secs":[{"id":"workspace","code":"custom","label":"Interview workspace","variant":"custom","formula":"Application workspace","p":{}}]},{"id":"catalyst","name":"Build control","route":"/catalyst","file":"apps/rust-desktop/src/build.rs","secs":[]}],"redirects":[],"footerLinks":[],"integrations":[],"files":list_files(&s.control)?,"scannedAt":chrono::Utc::now().to_rfc3339()}),
    ))
}
async fn roadmap(State(s): State<AppState>) -> Result<String> {
    let p = pipeline(&s.control).await?;
    let mut out = "# Storyfold roadmap\n\n".to_owned();
    for lane in ["Now", "Next", "Later"] {
        out.push_str(&format!("## {lane}\n\n"));
        for t in p["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|t| t["lane"] == lane)
        {
            out.push_str(&format!(
                "- [{}] {} ({})\n  Done when: {}\n",
                if t["status"] == "done" { "x" } else { " " },
                t["title"].as_str().unwrap_or(""),
                t["status"].as_str().unwrap_or(""),
                t["acceptance"].as_str().unwrap_or("")
            ));
        }
    }
    Ok(out)
}
async fn set_control(State(s): State<AppState>, Json(v): Json<Value>) -> Result<Json<Value>> {
    let _guard = s.control.lock.lock().await;
    let paused = v["paused"]
        .as_bool()
        .ok_or_else(|| AppError::bad("Choose pause or resume."))?;
    let out = json!({"paused":paused});
    write_json(&s.store.root.join(".catalyst/control.json"), &out).await?;
    Ok(Json(out))
}
fn task_patch(v: &Value) -> Result<Value> {
    let title = v["title"].as_str().unwrap_or("").trim();
    let acceptance = v["acceptance"].as_str().unwrap_or("").trim();
    let lane = v["lane"].as_str().unwrap_or("");
    let status = v["status"].as_str().unwrap_or("");
    if title.is_empty()
        || title.chars().count() > 160
        || acceptance.is_empty()
        || acceptance.len() > 2000
        || !["Now", "Next", "Later"].contains(&lane)
        || !["todo", "doing", "review", "done"].contains(&status)
    {
        return Err(AppError::bad("Task fields are invalid."));
    }
    secret_scan(&v.to_string())?;
    Ok(json!({"title":title,"acceptance":acceptance,"lane":lane,"status":status}))
}
async fn add_task(State(s): State<AppState>, Json(v): Json<Value>) -> Result<Json<Value>> {
    let mut patch = task_patch(&v)?;
    let _guard = s.control.lock.lock().await;
    let mut p = pipeline(&s.control).await?;
    patch["id"] = json!(uuid::Uuid::new_v4().to_string());
    patch["evidence"] = json!([]);
    p["tasks"]
        .as_array_mut()
        .ok_or_else(|| AppError::bad("Pipeline data is invalid."))?
        .push(patch);
    p["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    write_json(&s.store.root.join(".catalyst/pipeline.json"), &p).await?;
    Ok(Json(p))
}
async fn change_task(
    State(s): State<AppState>,
    Path(id): Path<String>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let patch = task_patch(&v)?;
    let _guard = s.control.lock.lock().await;
    let mut p = pipeline(&s.control).await?;
    let task = p["tasks"]
        .as_array_mut()
        .ok_or_else(|| AppError::bad("Pipeline data is invalid."))?
        .iter_mut()
        .find(|t| t["id"] == id)
        .ok_or_else(|| AppError::bad("Task does not exist."))?;
    for (k, v) in patch.as_object().unwrap() {
        task[k] = v.clone();
    }
    p["updatedAt"] = json!(chrono::Utc::now().to_rfc3339());
    write_json(&s.store.root.join(".catalyst/pipeline.json"), &p).await?;
    Ok(Json(p))
}
async fn make_proposal(c: &Control, v: Value) -> Result<Value> {
    let path = v["path"]
        .as_str()
        .ok_or_else(|| AppError::bad("Choose a source file."))?;
    let after = v["after"]
        .as_str()
        .ok_or_else(|| AppError::bad("Provide edited source."))?;
    let summary = v["summary"].as_str().unwrap_or("").trim();
    if after.len() > 512000
        || summary.is_empty()
        || summary.len() > 200
        || summary.chars().any(char::is_control)
    {
        return Err(AppError::bad(
            "Describe this change in one line up to 200 characters.",
        ));
    }
    let before = std::fs::read_to_string(safe_file(c, path)?)?;
    secret_scan(&before)?;
    secret_scan(after)?;
    secret_scan(summary)?;
    if v["hash"] != hash(&before) {
        return Err(AppError::conflict(
            "This file changed since you opened it. Reload before proposing an edit.",
        ));
    }
    if before == after {
        return Err(AppError::bad("There are no changes to review."));
    }
    if path.ends_with(".json") {
        serde_json::from_str::<Value>(after)?;
    }
    let out = json!({"id":uuid::Uuid::new_v4().to_string(),"path":path,"summary":summary,"before":before,"after":after,"hash":v["hash"],"status":"review","createdAt":chrono::Utc::now().to_rfc3339(),"undoOf":v["undoOf"]});
    write_json(&proposal_path(c, out["id"].as_str().unwrap())?, &out).await?;
    Ok(out)
}
async fn propose(State(s): State<AppState>, Json(v): Json<Value>) -> Result<Json<Value>> {
    let _guard = s.control.lock.lock().await;
    Ok(Json(make_proposal(&s.control, v).await?))
}
async fn discard(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>> {
    let _guard = s.control.lock.lock().await;
    let file = proposal_path(&s.control, &id)?;
    let mut p: Value = read_json(&file).await?;
    if p["status"] != "review" {
        return Err(AppError::bad("Only pending reviews can be discarded."));
    }
    p["status"] = json!("discarded");
    write_json(&file, &p).await?;
    Ok(Json(p))
}
async fn undo(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>> {
    let _guard = s.control.lock.lock().await;
    let p: Value = read_json(&proposal_path(&s.control, &id)?).await?;
    if p["status"] != "applied" {
        return Err(AppError::bad("Only applied changes can be undone."));
    }
    Ok(Json(make_proposal(&s.control,json!({"path":p["path"],"after":p["before"],"hash":hash(p["after"].as_str().unwrap_or("")),"summary":format!("Undo {}",p["summary"].as_str().unwrap_or("")).chars().take(200).collect::<String>(),"undoOf":id})).await?))
}
async fn apply(State(s): State<AppState>, Path(id): Path<String>) -> Result<Json<Value>> {
    let c = &s.control;
    let _guard = c.lock.lock().await;
    let entry = proposal_path(c, &id)?;
    let mut p: Value = read_json(&entry).await?;
    if p["status"] != "review" {
        return Err(AppError::bad(
            "This proposal is no longer waiting for review.",
        ));
    }
    let path = p["path"]
        .as_str()
        .ok_or_else(|| AppError::bad("Invalid proposal."))?
        .to_owned();
    let file = safe_file(c, &path)?;
    let before = std::fs::read_to_string(&file)?;
    let after = p["after"]
        .as_str()
        .ok_or_else(|| AppError::bad("Invalid proposal text."))?
        .to_owned();
    let summary = p["summary"].as_str().unwrap_or("Update source").to_owned();
    if hash(&before) != p["hash"] {
        return Err(AppError::conflict(
            "The source changed after this review was created. Create a new review.",
        ));
    }
    secret_scan(&before)?;
    secret_scan(&after)?;
    if !git(c, &["diff", "--cached", "--name-only", "--", &path], None)
        .await?
        .is_empty()
    {
        return Err(AppError::bad(
            "This file already has staged work. Commit or unstage it before applying this proposal.",
        ));
    }
    if git(c, &["ls-files", "--error-unmatch", "--", &path], None)
        .await
        .is_err()
    {
        return Err(AppError::bad(
            "Commit a new source file before editing it through Catalyst.",
        ));
    }
    if !git(
        c,
        &["ls-files", "--", ".gitattributes", "**/.gitattributes"],
        None,
    )
    .await?
    .is_empty()
    {
        return Err(AppError::bad(
            "Source commits with Git attribute filters are not supported.",
        ));
    }
    let info_attributes = git(c, &["rev-parse", "--git-path", "info/attributes"], None).await?;
    if c.store.root.join(info_attributes).exists() {
        return Err(AppError::bad(
            "Source commits with Git attribute filters are not supported.",
        ));
    }
    let index = private(c, "indexes")?.join(uuid::Uuid::new_v4().to_string());
    git(c, &["read-tree", "HEAD"], Some(&index)).await?;
    p["status"] = json!("applying");
    write_json(&entry, &p).await?;
    atomic_source(&file, &after)?;
    let commit = async {
        git(c, &["add", "--", &path], Some(&index)).await?;
        git(
            c,
            &["commit", "-m", &format!("catalyst: {summary}")],
            Some(&index),
        )
        .await?;
        git(c, &["rev-parse", "HEAD"], None).await
    }
    .await;
    let _ = std::fs::remove_file(&index);
    match commit {
        Ok(id) => {
            p["status"] = json!("applied");
            p["commit"] = json!(id);
            write_json(&entry, &p).await?;
            git(c, &["reset", "-q", "HEAD", "--", &path], None).await?;
            Ok(Json(p))
        }
        Err(error) => {
            if std::fs::read_to_string(&file)? == after {
                atomic_source(&file, &before)?;
            }
            p["status"] = json!("review");
            write_json(&entry, &p).await?;
            Err(error)
        }
    }
}
fn atomic_source(path: &FsPath, text: &str) -> Result<()> {
    use std::io::Write;
    let mut tmp = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| AppError::bad("Missing source folder."))?,
    )?;
    tmp.as_file()
        .set_permissions(std::fs::metadata(path)?.permissions())?;
    tmp.write_all(text.as_bytes())?;
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| AppError::from(e.error))?;
    Ok(())
}
fn redact(mut s: String) -> String {
    for (name, value) in std::env::vars() {
        if ["KEY", "TOKEN", "PASSWORD", "SECRET"]
            .iter()
            .any(|p| name.contains(p))
            && value.len() >= 8
        {
            s = s.replace(&value, "[redacted]");
        }
    }
    s
}
async fn start_check(State(s): State<AppState>, Json(v): Json<Value>) -> Result<Json<Value>> {
    let kind = v["kind"].as_str().unwrap_or("");
    if !["build", "test"].contains(&kind) {
        return Err(AppError::bad("Choose build or test."));
    }
    let c = &s.control;
    if paused(c).await?["paused"] == true {
        return Err(AppError::bad(
            "The local pipeline is paused. Resume it to run checks.",
        ));
    }
    let mut active = c.cancel.lock().await;
    if active.is_some() {
        return Err(AppError::conflict("Checks are already running."));
    }
    let token = CancellationToken::new();
    let args = if kind == "build" {
        vec!["build", "--workspace", "--locked"]
    } else {
        vec!["test", "--workspace", "--locked"]
    };
    let run = json!({"id":uuid::Uuid::new_v4().to_string(),"kind":kind,"status":"running","startedAt":chrono::Utc::now().to_rfc3339(),"output":format!("$ cargo {} --workspace --locked\n",kind)});
    let dir = s.store.root.join(".catalyst/runs");
    s.store.directory(&dir)?;
    let file = dir.join(format!("{}.json", run["id"].as_str().unwrap()));
    write_json(&file, &run).await?;
    *c.run.lock().await = Some(run.clone());
    let mut cmd = cargo_command(&s.store.root);
    cmd.current_dir(&s.store.root)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    for (key, _) in std::env::vars() {
        if ["KEY", "TOKEN", "PASSWORD", "SECRET"]
            .iter()
            .any(|p| key.contains(p))
        {
            cmd.env_remove(key);
        }
    }
    #[cfg(unix)]
    cmd.process_group(0);
    let c = c.clone();
    *active = Some(token.clone());
    drop(active);
    tokio::spawn(async move {
        let outcome = async {
            let mut child = cmd.spawn()?;
            #[cfg(unix)]
            let pid = child.id();
            let stdout = child.stdout.take().ok_or_else(|| AppError::bad("Could not read build output."))?;
            let stderr = child.stderr.take().ok_or_else(|| AppError::bad("Could not read build output."))?;
            let mut readers = Vec::new();
            for stream in [Box::pin(stdout) as std::pin::Pin<Box<dyn tokio::io::AsyncRead + Send>>, Box::pin(stderr)] {
                let c = c.clone();
                let file = file.clone();
                readers.push(tokio::spawn(async move {
                    let mut lines = BufReader::new(stream).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let mut current = c.run.lock().await;
                        if let Some(run) = current.as_mut() {
                            let output = redact(format!("{}{}\n", run["output"].as_str().unwrap_or(""), line));
                            run["output"] = json!(output.chars().rev().take(100000).collect::<String>().chars().rev().collect::<String>());
                            let _ = write_json(&file, run).await;
                        }
                    }
                }));
            }
            let (status, code) = tokio::select! {
                code = child.wait() => {
                    let code = code?;
                    (if code.success() { "passed" } else { "failed" }, code.code().unwrap_or(1))
                },
                _ = token.cancelled() => {
                    #[cfg(unix)]
                    if let Some(pid) = pid {
                        let _ = Command::new("kill").args(["-TERM", "--", &format!("-{pid}")]).status().await;
                    }
                    let _ = child.kill().await;
                    ("cancelled", 1)
                }
            };
            for reader in readers { let _ = reader.await; }
            Ok::<_, AppError>((status, code))
        }.await;
        if let Some(run) = c.run.lock().await.as_mut() {
            match outcome {
                Ok((status, code)) => {
                    run["status"] = json!(status);
                    run["exitCode"] = json!(code);
                }
                Err(_) => {
                    run["status"] = json!("failed");
                    run["output"] = json!(
                        "Cargo could not start. Check the Rust toolchain and project folder."
                    );
                }
            }
            run["endedAt"] = json!(chrono::Utc::now().to_rfc3339());
            let _ = write_json(&file, run).await;
        }
        *c.cancel.lock().await = None;
    });
    Ok(Json(run))
}
fn cargo_command(root: &FsPath) -> Command {
    for toolchain in [
        root.join("data/toolchain"),
        root.join("catalyst-solo/boilerplate/data/toolchain"),
    ] {
        let bin = toolchain.join("cargo/bin");
        let cargo = bin.join(if cfg!(windows) { "cargo.exe" } else { "cargo" });
        if cargo.is_file() {
            let mut cmd = Command::new(cargo);
            cmd.env("CARGO_HOME", toolchain.join("cargo"))
                .env("RUSTUP_HOME", toolchain.join("rustup"));
            let mut paths = vec![bin];
            paths.extend(std::env::split_paths(
                &std::env::var_os("PATH").unwrap_or_default(),
            ));
            if let Ok(path) = std::env::join_paths(paths) {
                cmd.env("PATH", path);
            }
            return cmd;
        }
    }
    Command::new("cargo")
}
async fn cancel_check(State(s): State<AppState>) -> Result<Json<Value>> {
    if let Some(token) = s.control.cancel.lock().await.as_ref() {
        token.cancel();
    }
    Ok(Json(
        s.control.run.lock().await.clone().unwrap_or(json!({})),
    ))
}
