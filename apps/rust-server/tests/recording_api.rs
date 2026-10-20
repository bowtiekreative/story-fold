use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use storyfold_server::{Store, api::router};
use tower::ServiceExt;
async fn call(
    app: Router,
    method: &str,
    path: &str,
    value: Value,
    token: Option<&str>,
    origin: Option<&str>,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:4311")
        .header("content-type", "application/json");
    if let Some(token) = token {
        req = req.header("x-storyfold-token", token);
    }
    if let Some(origin) = origin {
        req = req.header("origin", origin);
    }
    let response = app
        .oneshot(
            req.body(Body::from(serde_json::to_vec(&value).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
async fn recording_routes_enforce_tokens_revisions_and_device_boundaries() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::new(temp.path(), temp.path().join("data")).unwrap();
    let app = router(store.clone());
    let input = json!({"name":"Recording API check","orientation":"16:9"});
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/recordings",
            input.clone(),
            None,
            None
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (_, session) = call(app.clone(), "GET", "/api/session", Value::Null, None, None).await;
    let token = session["token"].as_str().unwrap();
    assert_eq!(
        call(
            app.clone(),
            "POST",
            "/api/recordings",
            input.clone(),
            Some(token),
            Some("https://example.invalid")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, created) = call(
        app.clone(),
        "POST",
        "/api/recordings",
        input,
        Some(token),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = created["id"].as_str().unwrap();
    let path = format!("/api/recordings/{id}");
    let edit = json!({"revision":0,"name":"A revised studio","orientation":"9:16","clips":[],"overlays":[{"id":"name","kind":"lowerThird","label":"Name","text":"Test speaker","secondary":"Local recording"}]});
    let (status, saved) = call(app.clone(), "PATCH", &path, edit.clone(), Some(token), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(saved["revision"], 1);
    assert_eq!(
        call(app.clone(), "PATCH", &path, edit, Some(token), None)
            .await
            .0,
        StatusCode::CONFLICT
    );
    let (status, _) = call(
        app.clone(),
        "POST",
        &format!("{path}/start"),
        json!({"cameraId":"https://example.invalid/private","microphoneId":null}),
        Some(token),
        None,
    )
    .await;
    assert!(!status.is_success());
    let (_, session) = call(app.clone(), "GET", &path, Value::Null, None, None).await;
    assert!(session["active"].is_null());
    assert!(session["takes"].as_array().unwrap().is_empty());
    assert_eq!(
        call(
            app.clone(),
            "GET",
            &format!("{path}/media/preview.jpg"),
            Value::Null,
            None,
            Some("https://example.invalid")
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let folder = store.data.join("recordings").join(id);
    assert!(!std::fs::read_dir(folder).unwrap().any(|e| {
        e.unwrap()
            .path()
            .extension()
            .is_some_and(|ext| ext == "mkv")
    }));
}
