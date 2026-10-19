use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use storyfold_core::Project;
use storyfold_server::{Store, api::router, storage::write_json};
use tower::ServiceExt;

async fn setup() -> (tempfile::TempDir, Store, Router, Project) {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::new(temp.path(), temp.path().join("data")).unwrap();
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../packages/storyfold-core/tests/fixtures/legacy.json"
    ))
    .unwrap();
    let p: Project = serde_json::from_value(fixture["project"].clone()).unwrap();
    store.save(&p).await.unwrap();
    std::fs::write(store.asset(&p.id, "proxy-A.mp4").unwrap(), b"0123456789").unwrap();
    let app = router(store.clone());
    (temp, store, app, p)
}
async fn request(
    app: Router,
    method: &str,
    uri: &str,
    body: Value,
    token: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(uri)
        .header("host", "127.0.0.1:4311")
        .header("content-type", "application/json");
    if let Some(t) = token {
        req = req.header("x-storyfold-token", t);
    }
    let res = app
        .oneshot(
            req.body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn token(app: Router) -> String {
    request(app, "GET", "/api/session", Value::Null, None)
        .await
        .1["token"]
        .as_str()
        .unwrap()
        .into()
}

#[tokio::test]
async fn revision_races_preserve_original_and_backups() {
    let (_temp, store, app, p) = setup().await;
    let token = token(app.clone()).await;
    let file = store.asset(&p.id, "project.json").unwrap();
    let original = [b"\n", std::fs::read(&file).unwrap().as_slice()].concat();
    std::fs::write(&file, &original).unwrap();
    let mut edit = serde_json::to_value(&p).unwrap();
    edit["name"] = json!("Saved Rust story");
    let (a, b) = tokio::join!(
        request(
            app.clone(),
            "PATCH",
            "/api/projects/fixture",
            edit.clone(),
            Some(&token)
        ),
        request(
            app.clone(),
            "PATCH",
            "/api/projects/fixture",
            edit,
            Some(&token)
        )
    );
    let mut statuses = [a.0.as_u16(), b.0.as_u16()];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    let saved = store.read("fixture").await.unwrap();
    assert_eq!(saved.revision, 1);
    assert_eq!(saved.name, "Saved Rust story");
    let backups: Vec<_> = std::fs::read_dir(store.project_dir("fixture").unwrap().join("backups"))
        .unwrap()
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        std::fs::read(backups[0].as_ref().unwrap().path()).unwrap(),
        original
    );
    let before: Project = serde_json::from_slice(&original).unwrap();
    let backup: Project =
        serde_json::from_slice(&std::fs::read(backups[0].as_ref().unwrap().path()).unwrap())
            .unwrap();
    assert_eq!(before, backup);
}
#[tokio::test]
async fn guards_reject_foreign_hosts_origins_tokens_and_private_paths() {
    let (_temp, store, app, p) = setup().await;
    for (host, origin) in [
        ("evil.example", "http://127.0.0.1:4311"),
        ("127.0.0.1:4311", "https://evil.example"),
    ] {
        let req = Request::builder()
            .uri("/api/session")
            .header("host", host)
            .header("origin", origin)
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            app.clone().oneshot(req).await.unwrap().status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        request(
            app.clone(),
            "PATCH",
            "/api/projects/fixture",
            json!(p),
            Some("expired")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let t = token(app.clone()).await;
    assert!(
        !request(
            app.clone(),
            "GET",
            "/api/catalyst/file?path=.env",
            Value::Null,
            None
        )
        .await
        .0
        .is_success()
    );
    assert!(
        !request(
            app,
            "GET",
            "/api/catalyst/file?path=docs%2F..%2F.env",
            Value::Null,
            Some(&t)
        )
        .await
        .0
        .is_success()
    );
    assert!(store.project_dir("../private").is_err());
    assert!(store.asset("fixture", "../secret").is_err());
}
#[tokio::test]
async fn media_ranges_and_read_only_project_loads_work() {
    let (_temp, store, app, _) = setup().await;
    let path = store.asset("fixture", "project.json").unwrap();
    let before = std::fs::read(&path).unwrap();
    assert_eq!(
        request(
            app.clone(),
            "GET",
            "/api/projects/fixture",
            Value::Null,
            None
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(std::fs::read(path).unwrap(), before);
    let req = Request::builder()
        .uri("/api/projects/fixture/media/A")
        .header("host", "127.0.0.1:4311")
        .header("origin", "http://dioxus.index.html")
        .header("range", "bytes=2-5")
        .body(Body::empty())
        .unwrap();
    let res = app.oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        &res.into_body().collect().await.unwrap().to_bytes()[..],
        b"2345"
    );
}
#[cfg(unix)]
#[tokio::test]
async fn linked_project_and_asset_paths_are_rejected() {
    use std::os::unix::fs::symlink;
    let (temp, store, _app, _) = setup().await;
    let outside = temp.path().join("outside");
    std::fs::create_dir(&outside).unwrap();
    symlink(&outside, store.data.join("projects/linked")).unwrap();
    assert!(store.project_dir("linked").is_err());
    symlink(
        temp.path().join("private"),
        store.project_dir("fixture").unwrap().join("linked.json"),
    )
    .unwrap();
    assert!(store.asset("fixture", "linked.json").is_err());
}
fn git(root: &std::path::Path, args: &[&str]) {
    assert!(
        std::process::Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    );
}
#[tokio::test]
async fn build_control_reviews_saves_and_undoes_source_without_touching_other_work() {
    let (temp, store, app, _) = setup().await;
    let root = temp.path();
    std::fs::create_dir(root.join("docs")).unwrap();
    std::fs::create_dir(root.join(".catalyst")).unwrap();
    std::fs::write(root.join(".gitignore"), "data/\n.catalyst/private/\n").unwrap();
    std::fs::write(root.join("docs/story.md"), "Original headline\n").unwrap();
    write_json(
        &root.join(".catalyst/pipeline.json"),
        &json!({"name":"Storyfold","tasks":[],"milestone":"Rust","message":"Review changes"}),
    )
    .await
    .unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.name", "Runtime test"]);
    git(root, &["config", "user.email", "runtime@example.invalid"]);
    git(root, &["add", "."]);
    git(root, &["commit", "-qm", "Initial fixture"]);
    let runs = root.join(".catalyst/runs");
    store.directory(&runs).unwrap();
    write_json(&runs.join("interrupted.json"), &json!({"id":"interrupted","status":"running","startedAt":"2026-10-18T00:00:00Z","output":""})).await.unwrap();
    let (status, dashboard) = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        request(app.clone(), "GET", "/api/catalyst", Value::Null, None),
    )
    .await
    .expect("Dashboard must not hold its run mutex while reading recent runs");
    assert_eq!(status, StatusCode::OK);
    assert_eq!(dashboard["runs"][0]["status"], "cancelled");
    std::fs::write(root.join("docs/unrelated.md"), "Keep this work\n").unwrap();
    git(root, &["add", "docs/unrelated.md"]);
    let token = token(app.clone()).await;
    let (status, source) = request(
        app.clone(),
        "GET",
        "/api/catalyst/file?path=docs%2Fstory.md",
        Value::Null,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let(status,proposal)=request(app.clone(),"POST","/api/catalyst/proposals",json!({"path":"docs/story.md","hash":source["hash"],"after":"A clearer headline\n","summary":"Improve heading"}),Some(&token)).await;
    assert!(status.is_success(), "{proposal}");
    assert_eq!(
        std::fs::read_to_string(root.join("docs/story.md")).unwrap(),
        "Original headline\n"
    );
    let (status, saved) = request(
        app.clone(),
        "POST",
        &format!(
            "/api/catalyst/proposals/{}/apply",
            proposal["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{saved}");
    assert_eq!(saved["status"], "applied");
    assert_eq!(
        std::fs::read_to_string(root.join("docs/story.md")).unwrap(),
        "A clearer headline\n"
    );
    let out = std::process::Command::new("git")
        .current_dir(root)
        .args(["diff", "--cached", "--name-only"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        "docs/unrelated.md"
    );
    let (status, undo) = request(
        app.clone(),
        "POST",
        &format!(
            "/api/catalyst/proposals/{}/undo",
            proposal["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = request(
        app.clone(),
        "POST",
        &format!(
            "/api/catalyst/proposals/{}/apply",
            undo["id"].as_str().unwrap()
        ),
        Value::Null,
        Some(&token),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        std::fs::read_to_string(root.join("docs/story.md")).unwrap(),
        "Original headline\n"
    );
    assert!(store.data.join("projects/fixture/project.json").exists());
}
