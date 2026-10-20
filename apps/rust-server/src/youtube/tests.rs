use super::*;
use axum::{
    body::Body,
    http::{Request as HttpRequest, StatusCode},
};
use http_body_util::BodyExt;
use serde_json::json;
use tower::ServiceExt;

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    input: Value,
) -> (StatusCode, Value) {
    let mut request = HttpRequest::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:4311")
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("x-storyfold-token", token);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(input.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&body)
            .unwrap_or_else(|_| json!({"error":String::from_utf8_lossy(&body)})),
    )
}
#[tokio::test]
async fn api_requires_session_explicit_account_and_keeps_credentials_out_of_responses() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::new(temp.path(), temp.path().join("data")).unwrap();
    let app = crate::api::router(store.clone());
    let (_, session) = call(&app, "GET", "/api/session", None, json!({})).await;
    let token = session["token"].as_str().unwrap();
    let (_, status) = call(&app, "GET", "/api/youtube/status", None, json!({})).await;
    assert_eq!(status["connected"], false);
    assert_eq!(status["configured"], false);
    let config = json!({"installed":{"client_id":"123456.apps.googleusercontent.com","client_secret":"synthetic-secret-never-display","auth_uri":"https://accounts.google.com/o/oauth2/auth","token_uri":"https://oauth2.googleapis.com/token","project_id":"example","redirect_uris":["http://localhost"]}});
    assert_eq!(
        call(&app, "POST", "/api/youtube/config", None, config.clone())
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (code, response) = call(
        &app,
        "POST",
        "/api/youtube/config",
        Some(token),
        config.clone(),
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{response}");
    assert_eq!(response["configured"], true);
    assert!(!response.to_string().contains("synthetic-secret"));
    assert!(!response.to_string().contains("googleusercontent.com"));
    let disk: Value = read_json(&store.data.join("youtube/client.json"))
        .await
        .unwrap();
    assert_eq!(disk["client_secret"], "synthetic-secret-never-display");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(store.data.join("youtube"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(store.data.join("youtube/client.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    let mut hostile = config;
    hostile["installed"]["token_uri"] = json!("http://127.0.0.1:4311/api/session");
    assert_eq!(
        call(&app, "POST", "/api/youtube/config", Some(token), hostile)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    // A web OAuth client cannot silently masquerade as an installed Desktop client.
    assert!(
        call(
            &app,
            "POST",
            "/api/youtube/config",
            Some(token),
            json!({"web":{}})
        )
        .await
        .0
        .is_client_error()
    );
    assert_eq!(call(&app,"POST","/api/youtube/imports",Some(token),json!({"recordingId":"studio","url":"http://127.0.0.1:4311/api/session","start":0,"end":5})).await.0,StatusCode::BAD_REQUEST);
    let (code, response) = call(
        &app,
        "POST",
        "/api/youtube/uploads",
        Some(token),
        json!({"recordingId":"studio","exportId":"export","channelId":"UC_reviewed","title":"Example","madeForKids":false}),
    )
    .await;
    assert!(code.is_client_error());
    assert!(!response.to_string().contains("synthetic-secret"));
    // A different window connected another channel after the upload review.
    // Local fake credentials exercise the guard without any Google request.
    let token_file = store.data.join("youtube/tokens.json");
    write_json(&token_file, &json!({"access_token":"fake-local-test","refresh_token":"fake-local-test","expires_at":chrono::Utc::now().timestamp()+3600,"scope":"https://www.googleapis.com/auth/youtube.upload https://www.googleapis.com/auth/youtube.readonly","channel":{"id":"UC_changed","title":"Changed test channel"}})).await.unwrap();
    let (code, response) = call(&app,"POST","/api/youtube/uploads",Some(token),json!({"recordingId":"studio","exportId":"export","channelId":"UC_reviewed","title":"Example","madeForKids":false})).await;
    assert_eq!(code, StatusCode::CONFLICT);
    assert!(
        response["error"]
            .as_str()
            .unwrap()
            .contains("channel changed")
    );
    // Remove the fake token before Disconnect, so its real revocation endpoint
    // is never contacted by this test.
    std::fs::remove_file(token_file).unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/youtube/disconnect",
            Some(token),
            json!({})
        )
        .await
        .0,
        StatusCode::OK
    );
}
#[tokio::test]
async fn interrupted_jobs_resume_identity_without_exposing_internal_upload_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let store = Store::new(temp.path(), temp.path().join("data")).unwrap();
    let s = AppState::new(store);
    let j = SavedJob {
        view: JobView {
            id: "job".into(),
            kind: "import".into(),
            recording_id: "studio".into(),
            status: "running".into(),
            stage: "Downloading".into(),
            progress: 23.,
            error: None,
            created_at: "2026-10-19T00:00:00Z".into(),
            video_id: None,
            url: None,
            bytes_sent: 100,
            bytes_total: 0,
        },
        request: Request::Import(ImportInput {
            recording_id: "studio".into(),
            url: "https://www.youtube.com/watch?v=BaW_jenozKc".into(),
            start: 0.,
            end: 1.,
        }),
        upload: None,
    };
    save(&s, &j).await.unwrap();
    let loaded = read(&s, "job").await.unwrap();
    assert_eq!(loaded.view.status, "error");
    assert_eq!(loaded.view.id, j.view.id);
    assert!(loaded.view.error.unwrap().contains("interrupted"));
    let exposed = serde_json::to_value(j.view).unwrap();
    assert!(exposed.get("request").is_none());
    assert!(exposed.get("upload").is_none());
    assert!(exposed.get("session").is_none());
}
