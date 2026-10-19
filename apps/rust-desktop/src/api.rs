use reqwest::{Client, Method, Response, Url, multipart};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct Api {
    client: Client,
    base: Url,
    token: Arc<Mutex<String>>,
}
impl Api {
    pub fn new(base: &str) -> Result<Self, String> {
        let base = local_url(base)?;
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            base,
            token: Arc::new(Mutex::new(String::new())),
        })
    }
    pub fn url(&self, path: &str) -> String {
        let mut url = self.base.clone();
        url.set_path(&format!("/api{}", path));
        url.to_string()
    }
    pub fn source_url(&self, file: &str) -> String {
        let mut url = self.base.clone();
        url.set_path("/api/catalyst/file");
        url.query_pairs_mut().append_pair("path", file);
        url.to_string()
    }
    async fn session(&self) -> Result<String, String> {
        let mut token = self.token.lock().await;
        if token.is_empty() {
            let response = self
                .client
                .get(self.url("/session"))
                .send()
                .await
                .map_err(network_error)?;
            let data: Value = decode(response).await?;
            *token = data["token"]
                .as_str()
                .filter(|s| !s.is_empty())
                .ok_or("The local server did not supply a session.")?
                .to_string();
        }
        Ok(token.clone())
    }
    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        decode(
            self.client
                .get(self.url(path))
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .map_err(network_error)?,
        )
        .await
    }
    pub async fn source<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        decode(
            self.client
                .get(self.source_url(path))
                .timeout(Duration::from_secs(30))
                .send()
                .await
                .map_err(network_error)?,
        )
        .await
    }
    pub async fn send<T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<T, String> {
        let token = self.session().await?;
        let mut request = self
            .client
            .request(method, self.url(path))
            .header("X-Storyfold-Token", token)
            .timeout(Duration::from_secs(120));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(network_error)?;
        if response.status() == reqwest::StatusCode::FORBIDDEN {
            self.token.lock().await.clear();
        }
        decode(response).await
    }
    pub async fn upload<T: DeserializeOwned>(
        &self,
        files: &[PathBuf],
        transcript: Option<&Path>,
        name: &str,
        manual: bool,
    ) -> Result<T, String> {
        if files.is_empty() || files.len() > 3 {
            return Err("Choose one to three camera videos.".into());
        }
        let mut form = multipart::Form::new()
            .text("name", name.to_owned())
            .text("grouping", if manual { "manual" } else { "auto" });
        for path in files {
            let size = tokio::fs::metadata(path)
                .await
                .map_err(|_| "The selected video is no longer available.")?
                .len();
            if size > 4 * 1024 * 1024 * 1024 {
                return Err("Each camera video must be smaller than 4 GB.".into());
            }
            // Part::file streams from disk; multi-gigabyte footage is never loaded into the UI heap.
            let part = multipart::Part::file(path)
                .await
                .map_err(|_| "Could not read a selected video.")?;
            form = form.part("files", part);
        }
        if let Some(path) = transcript {
            if tokio::fs::metadata(path)
                .await
                .map_err(|_| "The transcript is no longer available.")?
                .len()
                > 20_000_000
            {
                return Err("The optional transcript must be smaller than 20 MB.".into());
            }
            form = form.part(
                "transcript",
                multipart::Part::file(path)
                    .await
                    .map_err(|_| "Could not read the optional transcript.")?,
            );
        }
        let token = self.session().await?;
        decode(
            self.client
                .post(self.url("/projects"))
                .header("X-Storyfold-Token", token)
                .multipart(form)
                .send()
                .await
                .map_err(network_error)?,
        )
        .await
    }
    pub async fn download(&self, path: &str, destination: &Path) -> Result<(), String> {
        let mut response = self
            .client
            .get(self.url(path))
            .send()
            .await
            .map_err(network_error)?;
        if !response.status().is_success() {
            let _: Value = decode(response).await?;
            return Err("The export is unavailable.".into());
        }
        use tokio::io::AsyncWriteExt;
        let temporary = tempfile::NamedTempFile::new_in(
            destination.parent().ok_or("Choose an export folder.")?,
        )
        .map_err(|_| "Could not create that export file.")?;
        let mut output = tokio::fs::File::from_std(
            temporary
                .reopen()
                .map_err(|_| "Could not open the export file.")?,
        );
        while let Some(chunk) = response.chunk().await.map_err(network_error)? {
            output
                .write_all(&chunk)
                .await
                .map_err(|_| "Could not finish writing the export.")?;
        }
        output
            .flush()
            .await
            .map_err(|_| "Could not finish writing the export.")?;
        output
            .sync_all()
            .await
            .map_err(|_| "Could not finish writing the export.")?;
        drop(output);
        temporary
            .persist(destination)
            .map_err(|_| "Could not replace the chosen export file.")?;
        Ok(())
    }
}
fn network_error(_: reqwest::Error) -> String {
    "Cannot reach the local Storyfold service. Check that it is running, then retry.".into()
}
async fn decode<T: DeserializeOwned>(response: Response) -> Result<T, String> {
    let status = response.status();
    let value: Value = response
        .json()
        .await
        .map_err(|_| "The local service returned an invalid response.")?;
    if !status.is_success() {
        return Err(value["error"]
            .as_str()
            .unwrap_or("The request could not be completed.")
            .to_owned());
    }
    serde_json::from_value(value)
        .map_err(|_| "The local service returned data this version cannot read.".into())
}
fn local_url(base: &str) -> Result<Url, String> {
    let url = Url::parse(base).map_err(|_| "STORYFOLD_SERVER must be a loopback HTTP URL.")?;
    if url.scheme() != "http"
        || !matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || url.path() != "/"
    {
        return Err(
            "STORYFOLD_SERVER must be a loopback HTTP URL without credentials, query, or path."
                .into(),
        );
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn request(listener: &tokio::net::TcpListener, response: &str) -> String {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        let mut buffer = [0; 1024];
        loop {
            let count = stream.read(&mut buffer).await.unwrap();
            if count == 0 {
                break;
            }
            bytes.extend_from_slice(&buffer[..count]);
            if bytes.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        stream.write_all(response.as_bytes()).await.unwrap();
        String::from_utf8(bytes).unwrap()
    }
    #[test]
    fn rejects_remote_and_credential_urls() {
        for url in [
            "https://example.com",
            "http://192.168.1.1:4311",
            "http://user:pass@localhost:4311",
            "http://127.0.0.1:4311/path",
            "http://localhost:4311/?token=secret",
        ] {
            assert!(Api::new(url).is_err(), "{url}");
        }
        assert!(Api::new("http://127.0.0.1:4311").is_ok());
    }
    #[test]
    fn source_query_is_encoded_not_interpreted_as_a_request() {
        let api = Api::new("http://127.0.0.1:4311").unwrap();
        let url = Url::parse(&api.source_url("apps/a b.rs&token=no")).unwrap();
        assert_eq!(url.path(), "/api/catalyst/file");
        assert_eq!(
            url.query_pairs().collect::<Vec<_>>(),
            vec![("path".into(), "apps/a b.rs&token=no".into())]
        );
    }
    #[tokio::test]
    async fn mutation_obtains_local_token_and_surfaces_revision_conflict() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = Api::new(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move {
            let session=request(&listener,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: 24\r\n\r\n{\"token\":\"test-session\"}").await;
            assert!(session.starts_with("GET /api/session "));
            let body =
                "{\"error\":\"This project changed in another window. Reload before saving.\"}";
            let response = format!(
                "HTTP/1.1 409 Conflict\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
                body.len()
            );
            let mutation = request(&listener, &response).await;
            assert!(mutation.starts_with("PATCH /api/projects/demo "));
            assert!(
                mutation
                    .to_lowercase()
                    .contains("x-storyfold-token: test-session")
            );
        });
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            api.send::<Value>(
                Method::PATCH,
                "/projects/demo",
                Some(serde_json::json!({"revision":1})),
            ),
        )
        .await
        .unwrap();
        assert_eq!(
            result.unwrap_err(),
            "This project changed in another window. Reload before saving."
        );
        server.await.unwrap();
    }
    #[tokio::test]
    async fn interrupted_download_does_not_replace_existing_export() {
        let folder = tempfile::tempdir().unwrap();
        let destination = folder.path().join("story.mp4");
        tokio::fs::write(&destination, b"previous export")
            .await
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api = Api::new(&format!("http://{}", listener.local_addr().unwrap())).unwrap();
        let server = tokio::spawn(async move {
            request(
                &listener,
                "HTTP/1.1 200 OK\r\nConnection: close\r\nContent-Length: 100\r\n\r\npartial",
            )
            .await;
        });
        assert!(
            api.download("/projects/demo/exports/job/files/story.mp4", &destination)
                .await
                .is_err()
        );
        assert_eq!(
            tokio::fs::read(destination).await.unwrap(),
            b"previous export"
        );
        server.await.unwrap();
    }
}
