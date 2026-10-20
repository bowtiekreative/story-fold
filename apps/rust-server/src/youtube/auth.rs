use super::*;
use axum::{
    extract::Query,
    http::HeaderMap,
    response::{Html, IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde_json::json;
use sha2::{Digest, Sha256};

const AUTH: &str = "https://accounts.google.com/o/oauth2/v2/auth";
const TOKEN: &str = "https://oauth2.googleapis.com/token";
const SCOPES: &str = "https://www.googleapis.com/auth/youtube.upload https://www.googleapis.com/auth/youtube.readonly";
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct ClientConfig {
    client_id: String,
    client_secret: String,
    auth_uri: String,
    token_uri: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ConfigFile {
    installed: ClientConfig,
}
#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Channel {
    pub id: String,
    pub title: String,
}
#[derive(Serialize, Deserialize)]
struct Tokens {
    access_token: String,
    refresh_token: String,
    expires_at: i64,
    scope: String,
    channel: Channel,
}
fn path(s: &AppState, name: &str) -> Result<PathBuf> {
    let dir = base(&s.store)?;
    // Existing installations are tightened too, not merely new directories.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700))?;
    }
    let p = dir.join(name);
    reject_link(&p)?;
    Ok(p)
}
fn valid_config(c: &ClientConfig) -> Result<()> {
    if ![AUTH, "https://accounts.google.com/o/oauth2/auth"].contains(&c.auth_uri.as_str())
        || c.token_uri != TOKEN
        || !c.client_id.ends_with(".apps.googleusercontent.com")
        || c.client_id.len() > 250
        || c.client_id.len() < 25
        || !c
            .client_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        || c.client_secret.is_empty()
        || c.client_secret.len() > 512
        || c.client_secret.chars().any(char::is_control)
    {
        return Err(AppError::bad(
            "Choose the original Google OAuth Desktop app JSON with the standard Google authorization and token endpoints.",
        ));
    }
    Ok(())
}
async fn config(s: &AppState) -> Result<ClientConfig> {
    let c = read_json(&path(s, "client.json")?).await.map_err(|_| {
        AppError::bad("Choose a Google OAuth Desktop app client JSON in YouTube settings first.")
    })?;
    valid_config(&c)?;
    Ok(c)
}
async fn tokens(s: &AppState) -> Result<Tokens> {
    let t: Tokens = read_json(&path(s, "tokens.json")?)
        .await
        .map_err(|_| AppError::bad("Connect your YouTube channel first."))?;
    if t.access_token.is_empty()
        || t.access_token.len() > 16384
        || t.refresh_token.is_empty()
        || t.refresh_token.len() > 16384
        || !granted(&t.scope)
    {
        return Err(AppError::bad(
            "Reconnect your YouTube channel; its saved credentials are incomplete.",
        ));
    }
    Ok(t)
}
fn granted(scope: &str) -> bool {
    SCOPES
        .split_whitespace()
        .all(|required| scope.split_whitespace().any(|v| v == required))
}
pub(super) async fn status(s: &AppState) -> Result<Value> {
    let configured = config(s).await.is_ok();
    let account = tokens(s).await.ok();
    let connecting = s
        .youtube
        .pending
        .lock()
        .await
        .as_ref()
        .is_some_and(|c| !c.is_cancelled());
    Ok(
        json!({"configured":configured,"connected":configured && account.is_some(),"channel":account.map(|a|a.channel),"downloaderAvailable":download::available(&s.store),"connecting":connecting,"message":s.youtube.notice.lock().await.clone()}),
    )
}
pub(super) async fn configure(
    State(s): State<AppState>,
    Json(input): Json<ConfigFile>,
) -> Result<Json<Value>> {
    valid_config(&input.installed)?;
    let _guard = s.youtube.auth_lock.lock().await;
    if !s.youtube.active.lock().await.is_empty() {
        return Err(AppError::conflict(
            "Finish or cancel YouTube jobs before changing the OAuth client.",
        ));
    }
    if path(&s, "tokens.json")?.exists() {
        return Err(AppError::conflict(
            "Disconnect the current channel before replacing the Google client.",
        ));
    }
    if let Some(c) = s.youtube.pending.lock().await.take() {
        c.cancel();
    }
    write_json(&path(&s, "client.json")?, &input.installed).await?;
    *s.youtube.notice.lock().await = None;
    Ok(Json(status(&s).await?))
}
#[derive(Clone)]
struct Callback {
    s: AppState,
    expected: String,
    verifier: String,
    redirect: String,
    config: ClientConfig,
    cancel: CancellationToken,
    used: Arc<Mutex<bool>>,
    host: String,
}
#[derive(Deserialize)]
struct Reply {
    #[serde(default)]
    state: String,
    code: Option<String>,
    error: Option<String>,
}
fn random() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn constant_equal(a: &str, b: &str) -> bool {
    let mut diff = a.len() ^ b.len();
    for (a, b) in a.bytes().zip(b.bytes()) {
        diff |= (a ^ b) as usize;
    }
    diff == 0
}
fn authorization(c: &ClientConfig, state: &str, verifier: &str, redirect: &str) -> Result<String> {
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = reqwest::Url::parse(AUTH)
        .map_err(|_| AppError::bad("Google authorization is unavailable."))?;
    url.query_pairs_mut().extend_pairs([
        ("client_id", c.client_id.as_str()),
        ("redirect_uri", redirect),
        ("response_type", "code"),
        ("scope", SCOPES),
        ("state", state),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("access_type", "offline"),
        ("prompt", "consent"),
    ]);
    Ok(url.into())
}
pub(super) async fn connect(State(s): State<AppState>) -> Result<Json<Value>> {
    let _guard = s.youtube.auth_lock.lock().await;
    if !s.youtube.active.lock().await.is_empty() {
        return Err(AppError::conflict(
            "Finish or cancel YouTube jobs before connecting a channel.",
        ));
    }
    let config = config(&s).await?;
    if let Some(old) = s.youtube.pending.lock().await.take() {
        old.cancel();
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let host = listener.local_addr()?.to_string();
    let redirect = format!("http://{host}/");
    let expected = random();
    let verifier = random();
    let cancel = CancellationToken::new();
    let url = authorization(&config, &expected, &verifier, &redirect)?;
    let state = Callback {
        s: s.clone(),
        expected,
        verifier,
        redirect,
        config,
        cancel: cancel.clone(),
        used: Default::default(),
        host,
    };
    *s.youtube.pending.lock().await = Some(cancel.clone());
    *s.youtube.notice.lock().await = None;
    let router = Router::new().route("/", get(callback)).with_state(state);
    tokio::spawn(async move {
        let timer = cancel.clone();
        let notice = s.youtube.notice.clone();
        let shutdown = async move {
            tokio::select! { _=timer.cancelled()=>{}, _=tokio::time::sleep(Duration::from_secs(300))=>{ *notice.lock().await=Some("Google sign-in timed out. Click Connect to try again.".into()); timer.cancel(); } }
        };
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(shutdown)
            .await;
    });
    Ok(Json(json!({"url":url})))
}
async fn callback(
    State(c): State<Callback>,
    headers: HeaderMap,
    Query(reply): Query<Reply>,
) -> Response {
    if headers.get("host").and_then(|v| v.to_str().ok()) != Some(c.host.as_str())
        || !constant_equal(&reply.state, &c.expected)
        || c.cancel.is_cancelled()
    {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Html("Invalid or expired sign-in. Return to Storyfold and click Connect."),
        )
            .into_response();
    }
    let mut used = c.used.lock().await;
    if *used {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Html("This sign-in response was already used."),
        )
            .into_response();
    }
    *used = true;
    drop(used);
    let result=async {
        if reply.error.is_some() {return Err(AppError::bad("Google sign-in was declined. You can reconnect when ready."));}
        let code=reply.code.filter(|code|!code.is_empty() && code.len()<=4096).ok_or_else(||AppError::bad("Google did not return a sign-in code. Reconnect to try again."))?;
        let _guard=c.s.youtube.auth_lock.lock().await;
        cancelled(&c.cancel)?;
        let response=client()?.post(TOKEN).form(&[("client_id",c.config.client_id.as_str()),("client_secret",&c.config.client_secret),("code",&code),("code_verifier",&c.verifier),("grant_type","authorization_code"),("redirect_uri",&c.redirect)]).send().await?;
        let status=response.status();let data=bounded_json(response).await?;
        if !status.is_success() {return Err(AppError::bad("Google could not complete sign-in. Check the Desktop OAuth client and consent settings, then reconnect."));}
        let access=field(&data,"access_token")?;let refresh=field(&data,"refresh_token")?;
        let scope=data["scope"].as_str().unwrap_or("");
        if !granted(scope) {return Err(AppError::bad("Allow both YouTube upload and channel read access when reconnecting."));}
        let channel=channel(&access).await?;
        let tokens=Tokens{access_token:access,refresh_token:refresh,expires_at:expiry(&data),scope:scope.into(),channel};
        cancelled(&c.cancel)?;write_json(&path(&c.s,"tokens.json")?,&tokens).await?;
        Ok(())
    }.await;
    let text = if let Err(e) = result {
        *c.s.youtube.notice.lock().await = Some(e.message);
        "YouTube sign-in could not finish. Return to Storyfold for the reason and try again."
    } else {
        *c.s.youtube.notice.lock().await =
            Some("YouTube channel connected. Return to Storyfold to review your upload.".into());
        "YouTube connected. You can close this window and return to Storyfold."
    };
    c.cancel.cancel();
    let mut response = Html(format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Storyfold YouTube</title><p>{text}</p>"
    ))
    .into_response();
    for (key, value) in [
        ("cache-control", "no-store"),
        ("referrer-policy", "no-referrer"),
        (
            "content-security-policy",
            "default-src 'none'; frame-ancestors 'none'",
        ),
        ("x-content-type-options", "nosniff"),
    ] {
        response.headers_mut().insert(
            axum::http::HeaderName::from_static(key),
            axum::http::HeaderValue::from_static(value),
        );
    }
    response
}
fn field(data: &Value, key: &str) -> Result<String> {
    data[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= 16384)
        .map(str::to_owned)
        .ok_or_else(|| {
            AppError::bad("Google returned incomplete credentials. Reconnect your channel.")
        })
}
fn expiry(v: &Value) -> i64 {
    chrono::Utc::now().timestamp() + v["expires_in"].as_i64().unwrap_or(3600).clamp(60, 86400)
}
async fn channel(access: &str) -> Result<Channel> {
    let response = client()?
        .get("https://www.googleapis.com/youtube/v3/channels")
        .query(&[("part", "snippet"), ("mine", "true")])
        .bearer_auth(access)
        .send()
        .await?;
    let status = response.status();
    let v = bounded_json(response).await?;
    if !status.is_success() {
        return Err(AppError::bad(
            "YouTube channel access failed. Enable YouTube Data API v3 for this client and reconnect.",
        ));
    }
    let items=v["items"].as_array().filter(|items|items.len()==1).ok_or_else(||AppError::bad("This Google account has no single selected YouTube channel. Create or select your channel and reconnect."))?;
    let item = &items[0];
    let id = item["id"]
        .as_str()
        .filter(|s| {
            s.len() <= 100
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
        })
        .ok_or_else(|| AppError::bad("YouTube returned an invalid channel."))?;
    let title = item["snippet"]["title"]
        .as_str()
        .unwrap_or("YouTube channel")
        .chars()
        .take(200)
        .collect();
    Ok(Channel {
        id: id.into(),
        title,
    })
}
pub(super) async fn access(s: &AppState) -> Result<(String, Channel)> {
    let _guard = s.youtube.auth_lock.lock().await;
    let config = config(s).await?;
    let mut t = tokens(s).await?;
    if t.expires_at <= chrono::Utc::now().timestamp() + 60 {
        let response = client()?
            .post(TOKEN)
            .form(&[
                ("client_id", config.client_id.as_str()),
                ("client_secret", &config.client_secret),
                ("refresh_token", &t.refresh_token),
                ("grant_type", "refresh_token"),
            ])
            .send()
            .await?;
        let status = response.status();
        let data = bounded_json(response).await?;
        if !status.is_success() {
            return Err(AppError::bad(
                "Your Google permission expired or was revoked. Reconnect your YouTube channel.",
            ));
        }
        t.access_token = field(&data, "access_token")?;
        t.expires_at = expiry(&data);
        if let Some(scope) = data["scope"].as_str() {
            if !granted(scope) {
                return Err(AppError::bad(
                    "Reconnect and allow upload plus channel access.",
                ));
            }
            t.scope = scope.into();
        }
        write_json(&path(s, "tokens.json")?, &t).await?;
    }
    Ok((t.access_token, t.channel))
}
pub(super) async fn invalidate_access(s: &AppState) -> Result<()> {
    let _guard = s.youtube.auth_lock.lock().await;
    let mut t = tokens(s).await?;
    t.expires_at = 0;
    write_json(&path(s, "tokens.json")?, &t).await
}
pub(super) async fn disconnect(State(s): State<AppState>) -> Result<Json<Value>> {
    let _guard = s.youtube.auth_lock.lock().await;
    if let Some(c) = s.youtube.pending.lock().await.take() {
        c.cancel();
    }
    for c in s.youtube.active.lock().await.values() {
        c.cancel();
    }
    let file = path(&s, "tokens.json")?;
    let saved = tokens(&s).await.ok();
    if file.exists() {
        std::fs::remove_file(file)?;
    }
    let revoked = if let Some(t) = saved {
        tokio::time::timeout(Duration::from_secs(10), async {
            client()?
                .post("https://oauth2.googleapis.com/revoke")
                .form(&[("token", t.refresh_token)])
                .send()
                .await
                .map_err(AppError::from)
        })
        .await
        .is_ok_and(|r| r.is_ok_and(|r| r.status().is_success()))
    } else {
        true
    };
    *s.youtube.notice.lock().await=Some(if revoked{"Disconnected from YouTube."}else{"Disconnected locally. Google could not confirm revocation; you can also remove Storyfold in your Google account permissions."}.into());
    Ok(Json(status(&s).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pkce_state_and_redirect_are_bound_to_google() {
        let c = ClientConfig {
            client_id: "123456.apps.googleusercontent.com".into(),
            client_secret: "test-only".into(),
            auth_uri: AUTH.into(),
            token_uri: TOKEN.into(),
        };
        valid_config(&c).unwrap();
        let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
        let url = reqwest::Url::parse(
            &authorization(&c, "state-test", verifier, "http://127.0.0.1:12345/").unwrap(),
        )
        .unwrap();
        let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(
            pairs["code_challenge"],
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(pairs["code_challenge_method"], "S256");
        assert_eq!(pairs["state"], "state-test");
        assert_eq!(pairs["redirect_uri"], "http://127.0.0.1:12345/");
        assert!(constant_equal("same", "same"));
        assert!(!constant_equal("same", "other"));
        assert!(!constant_equal("same", "same-longer"));
        let mut bad = c.clone();
        bad.token_uri = "http://127.0.0.1/".into();
        assert!(valid_config(&bad).is_err());
        assert!(!granted("https://www.googleapis.com/auth/youtube.readonly"));
    }
    #[tokio::test]
    async fn loopback_callback_rejects_wrong_state_and_consumes_denial_without_external_requests() {
        let temp = tempfile::tempdir().unwrap();
        let store = Store::new(temp.path(), temp.path().join("data")).unwrap();
        let s = AppState::new(store);
        let config = ClientConfig {
            client_id: "123456.apps.googleusercontent.com".into(),
            client_secret: "fake-local-test".into(),
            auth_uri: AUTH.into(),
            token_uri: TOKEN.into(),
        };
        let _ = configure(State(s.clone()), Json(ConfigFile { installed: config }))
            .await
            .unwrap();
        let Json(response) = connect(State(s.clone())).await.unwrap();
        let auth = reqwest::Url::parse(response["url"].as_str().unwrap()).unwrap();
        let params: std::collections::HashMap<_, _> = auth.query_pairs().into_owned().collect();
        let redirect = &params["redirect_uri"];
        assert!(redirect.starts_with("http://127.0.0.1:"));
        let http = client().unwrap();
        let bad = http
            .get(redirect)
            .query(&[("state", "wrong"), ("error", "access_denied")])
            .send()
            .await
            .unwrap();
        assert_eq!(bad.status(), axum::http::StatusCode::BAD_REQUEST);
        assert_eq!(status(&s).await.unwrap()["connecting"], true);
        let denied = http
            .get(redirect)
            .query(&[
                ("state", params["state"].as_str()),
                ("error", "access_denied"),
            ])
            .send()
            .await
            .unwrap();
        assert!(denied.status().is_success());
        assert_eq!(denied.headers()["referrer-policy"], "no-referrer");
        let state = status(&s).await.unwrap();
        assert_eq!(state["connected"], false);
        assert_eq!(state["connecting"], false);
        assert!(state["message"].as_str().unwrap().contains("declined"));
        assert!(!path(&s, "tokens.json").unwrap().exists());
    }
}
