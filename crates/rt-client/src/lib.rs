//! Blocking HTTP client of the review-tools JSON API, with endpoint and token discovery.
//!
//! The endpoint comes from, in order: `RT_ENDPOINT` (with `RT_TOKEN`); the config file's
//! entry for the repository the current directory is in (`[repos."<path>"]`, written by
//! `rt init`); or the config file's `endpoint` and `token`.

pub mod paths;
pub mod state;

use std::path::PathBuf;
use std::time::Duration;

use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use rt_api::*;
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Header that marks a request as the agent's (see the server's `ACTOR_HEADER`).
const ACTOR_HEADER: &str = "x-rt-actor";
/// Header required on state-changing requests (see the server's `CLIENT_HEADER`).
const CLIENT_HEADER: &str = "x-rt-client";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointSource {
    Env,
    ConfigFile(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Endpoint {
    /// Base URL without a trailing slash, such as `https://rt.example.com`.
    pub url: String,
    pub token: Option<String>,
    pub source: EndpointSource,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("cannot reach {url}: {message}")]
    Unreachable { url: String, message: String },
    #[error("{message}")]
    Api {
        status: u16,
        code: String,
        message: String,
    },
    #[error("{0}")]
    Other(String),
}

impl ClientError {
    pub fn code(&self) -> Option<&str> {
        match self {
            ClientError::Api { code, .. } => Some(code),
            _ => None,
        }
    }
}

pub type Result<T> = std::result::Result<T, ClientError>;

#[derive(serde::Deserialize, Default)]
struct ConfigFile {
    endpoint: Option<String>,
    token: Option<String>,
    /// Per repository, by the path of its top directory.
    #[serde(default)]
    repos: std::collections::BTreeMap<String, RepoConfig>,
}

#[derive(serde::Deserialize, Default)]
struct RepoConfig {
    endpoint: Option<String>,
    token: Option<String>,
}

/// The top directory of the git repository `dir` is in: the nearest one with `.git`.
pub fn repo_top(dir: &std::path::Path) -> Option<PathBuf> {
    let dir = dir.canonicalize().ok()?;
    dir.ancestors()
        .find(|d| d.join(".git").exists())
        .map(std::path::Path::to_path_buf)
}

/// Remember the instance and token for a repository (`rt init`), in the config file, which
/// only its owner can read. Returns the file's path.
pub fn save_repo_config(repo: &std::path::Path, endpoint: &str, token: &str) -> Result<PathBuf> {
    let path = paths::config_file();
    let fail =
        |e: &dyn std::fmt::Display| ClientError::Other(format!("writing {}: {e}", path.display()));
    let mut table: toml::Table = match paths::read_own(&path) {
        Some(text) => toml::from_str(&text).map_err(|e| fail(&e))?,
        None => toml::Table::new(),
    };
    let repos = table
        .entry("repos")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let Some(repos) = repos.as_table_mut() else {
        return Err(fail(&"`repos` is not a table"));
    };
    let mut entry = toml::Table::new();
    entry.insert("endpoint".into(), endpoint.trim_end_matches('/').into());
    entry.insert("token".into(), token.into());
    repos.insert(repo.display().to_string(), toml::Value::Table(entry));
    let text = toml::to_string(&table).map_err(|e| fail(&e))?;
    if !paths::config_is_private() {
        return Err(fail(
            &"no configuration directory of your own to keep a token in; set RT_HOME",
        ));
    }
    if let Some(dir) = path.parent() {
        paths::private_dir(dir).map_err(|e| fail(&e))?;
    }
    let tmp = path.with_extension("toml.tmp");
    {
        use std::io::Write;
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
        let mut file = options.open(&tmp).map_err(|e| fail(&e))?;
        file.write_all(text.as_bytes()).map_err(|e| fail(&e))?;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| fail(&e))?;
    }
    std::fs::rename(&tmp, &path).map_err(|e| fail(&e))?;
    Ok(path)
}

/// Whether a token may go to `base`: over HTTPS, or plain HTTP to this machine only.
/// Anything else would show it to everyone on the way.
pub fn may_carry_token(base: &str) -> bool {
    if base.starts_with("https://") {
        return true;
    }
    let Some(rest) = base.strip_prefix("http://") else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let name = match host.strip_prefix('[') {
        Some(v6) => v6.split(']').next().map(|h| format!("[{h}]")),
        None => host.split(':').next().map(str::to_owned),
    };
    matches!(name.as_deref(), Some("127.0.0.1" | "localhost" | "[::1]")) && !host.contains('@')
}

/// Where the API is, without contacting it. Refuses a token configured for a plain HTTP
/// address that is not on this machine.
pub fn discover() -> Result<Endpoint> {
    let endpoint = discover_unchecked()?.ok_or_else(|| {
        ClientError::Other(
            "no review-tools instance for this repository: run `rt init` in it, or set RT_ENDPOINT (and RT_TOKEN)"
                .into(),
        )
    })?;
    if endpoint.token.is_some() && !may_carry_token(&endpoint.url) {
        return Err(ClientError::Other(format!(
            "{} is plain HTTP: rt sends its token only over HTTPS (or to this machine); use the https:// address",
            endpoint.url
        )));
    }
    Ok(endpoint)
}

fn discover_unchecked() -> Result<Option<Endpoint>> {
    if let Some(url) = std::env::var("RT_ENDPOINT").ok().filter(|s| !s.is_empty()) {
        let token = std::env::var("RT_TOKEN").ok().filter(|s| !s.is_empty());
        return Ok(Some(Endpoint {
            url: url.trim_end_matches('/').to_owned(),
            token,
            source: EndpointSource::Env,
        }));
    }
    let config_path = paths::config_file();
    if let Some(text) = paths::read_own(&config_path) {
        let config: ConfigFile = toml::from_str(&text)
            .map_err(|e| ClientError::Other(format!("reading {}: {e}", config_path.display())))?;
        let repo = std::env::current_dir().ok().and_then(|d| repo_top(&d));
        let for_repo = repo.and_then(|r| config.repos.get(&r.display().to_string()));
        if let Some(RepoConfig {
            endpoint: Some(url),
            token,
        }) = for_repo
        {
            return Ok(Some(Endpoint {
                url: url.trim_end_matches('/').to_owned(),
                token: token.clone(),
                source: EndpointSource::ConfigFile(config_path),
            }));
        }
        if let Some(url) = config.endpoint {
            return Ok(Some(Endpoint {
                url: url.trim_end_matches('/').to_owned(),
                token: config.token,
                source: EndpointSource::ConfigFile(config_path),
            }));
        }
    }
    Ok(None)
}

/// Percent-encode a value as one path segment.
pub fn segment(value: &str) -> String {
    const SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~');
    utf8_percent_encode(value, SEGMENT).to_string()
}

/// The system's trusted roots (honouring `SSL_CERT_FILE` and `SSL_CERT_DIR`), or Mozilla's
/// built-in set where the system has none. The system's matter where a proxy re-signs
/// HTTPS traffic with its own authority, as the egress proxy of Claude Code cloud
/// environments does to add network secrets.
fn root_certs() -> &'static ureq::tls::RootCerts {
    static ROOTS: std::sync::OnceLock<ureq::tls::RootCerts> = std::sync::OnceLock::new();
    ROOTS.get_or_init(|| {
        let native = rustls_native_certs::load_native_certs();
        let certs: Vec<_> = native
            .certs
            .iter()
            .map(|c| ureq::tls::Certificate::from_der(c.as_ref()).to_owned())
            .collect();
        if certs.is_empty() {
            ureq::tls::RootCerts::WebPki
        } else {
            ureq::tls::RootCerts::new_with_certs(&certs)
        }
    })
}

pub struct Client {
    agent: ureq::Agent,
    base: String,
    token: Option<String>,
    as_agent: bool,
}

impl Client {
    pub fn new(base: &str, token: Option<String>) -> Self {
        let tls = ureq::tls::TlsConfig::builder()
            .root_certs(root_certs().clone())
            .build();
        let agent = ureq::Agent::config_builder()
            .tls_config(tls)
            .http_status_as_error(false)
            .timeout_connect(Some(Duration::from_secs(5)))
            .timeout_global(Some(Duration::from_secs(60)))
            // A redirect could take the token to another host; the API never redirects
            // its callers, so a redirect is reported instead.
            .max_redirects(0)
            .user_agent(concat!("rt/", env!("CARGO_PKG_VERSION")))
            .build()
            .new_agent();
        Client {
            agent,
            base: base.trim_end_matches('/').to_owned(),
            token,
            as_agent: false,
        }
    }

    /// Act as the agent: messages are attributed to the agent.
    pub fn as_agent(mut self) -> Self {
        self.as_agent = true;
        self
    }

    pub fn base(&self) -> &str {
        &self.base
    }

    /// The web page of a session.
    pub fn session_url(&self, id: &str) -> String {
        format!("{}/sessions/{}", self.base, segment(id))
    }

    fn url(&self, path: &str) -> String {
        format!("{}/api/v1{path}", self.base)
    }

    fn decorate<B>(&self, mut req: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        req = req.header(CLIENT_HEADER, "rt-cli");
        if self.as_agent {
            req = req.header(ACTOR_HEADER, "agent");
        }
        if let Some(token) = &self.token
            && may_carry_token(&self.base)
        {
            // Not Authorization: an instance behind CloudFront sees only what its standard
            // policies pass on, which includes X-Rt-Token.
            req = req.header("x-rt-token", token.as_str());
        }
        req
    }

    fn unreachable(&self, e: ureq::Error) -> ClientError {
        match e {
            ureq::Error::Io(_)
            | ureq::Error::ConnectionFailed
            | ureq::Error::HostNotFound
            | ureq::Error::Timeout(_) => ClientError::Unreachable {
                url: self.base.clone(),
                message: e.to_string(),
            },
            e => ClientError::Other(format!("request to {} failed: {e}", self.base)),
        }
    }

    fn finish<T: DeserializeOwned>(
        &self,
        response: std::result::Result<ureq::http::Response<ureq::Body>, ureq::Error>,
    ) -> Result<T> {
        let mut response = response.map_err(|e| self.unreachable(e))?;
        let status = response.status().as_u16();
        let body = response
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_vec()
            .map_err(|e| self.unreachable(e))?;
        if (300..400).contains(&status) {
            let to = response
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("elsewhere");
            return Err(ClientError::Api {
                status,
                code: "redirected".into(),
                message: format!(
                    "{} redirects to {to}; rt follows no redirects with its token: set the endpoint to the address it should use",
                    self.base
                ),
            });
        }
        if (200..300).contains(&status) {
            let body = if body.is_empty() {
                b"null".to_vec()
            } else {
                body
            };
            return serde_json::from_slice(&body).map_err(|e| {
                ClientError::Other(format!("unexpected response from the server: {e}"))
            });
        }
        match serde_json::from_slice::<ErrorBody>(&body) {
            Ok(e) => Err(ClientError::Api {
                status,
                code: e.error.code,
                message: e.error.message,
            }),
            Err(_) => Err(ClientError::Api {
                status,
                code: "http_error".into(),
                message: format!("HTTP {status}: {}", String::from_utf8_lossy(&body).trim()),
            }),
        }
    }

    fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T> {
        self.get_with_timeout(path, None)
    }

    fn get_with_timeout<T: DeserializeOwned>(
        &self,
        path: &str,
        timeout: Option<Duration>,
    ) -> Result<T> {
        let mut req = self.decorate(self.agent.get(self.url(path)));
        if let Some(t) = timeout {
            req = req.config().timeout_global(Some(t)).build();
        }
        self.finish(req.call())
    }

    fn send<B: Serialize, T: DeserializeOwned>(
        &self,
        method: &str,
        path: &str,
        body: &B,
    ) -> Result<T> {
        let url = self.url(path);
        let req = match method {
            "POST" => self.agent.post(url),
            "PUT" => self.agent.put(url),
            "PATCH" => self.agent.patch(url),
            _ => unreachable!("method {method}"),
        };
        self.finish(self.decorate(req).send_json(body))
    }

    pub fn health(&self) -> Result<Health> {
        let req = self
            .decorate(self.agent.get(self.url("/health")))
            .config()
            .timeout_global(Some(Duration::from_secs(2)))
            .build();
        self.finish(req.call())
    }

    /// `rt init`: exchange the browser's one-time code for a token. Needs no token.
    pub fn connect_exchange(&self, req: &ConnectExchange) -> Result<Connected> {
        self.send("POST", "/connect/exchange", req)
    }

    pub fn whoami(&self) -> Result<Whoami> {
        self.get("/whoami")
    }

    // Projects and work packages

    pub fn projects(&self) -> Result<Vec<Project>> {
        self.get("/projects")
    }

    pub fn work_packages(&self, project: &str) -> Result<Vec<WorkPackage>> {
        self.get(&format!("/projects/{}/work-packages", segment(project)))
    }

    pub fn create_work_package(
        &self,
        project: &str,
        req: &CreateWorkPackage,
    ) -> Result<WorkPackage> {
        self.send(
            "POST",
            &format!("/projects/{}/work-packages", segment(project)),
            req,
        )
    }

    pub fn work_package(&self, project: &str, wp: &str) -> Result<WorkPackageDetail> {
        self.get(&format!(
            "/projects/{}/work-packages/{}",
            segment(project),
            segment(wp)
        ))
    }

    pub fn update_work_package(
        &self,
        project: &str,
        wp: &str,
        req: &UpdateWorkPackage,
    ) -> Result<WorkPackage> {
        self.send(
            "PATCH",
            &format!(
                "/projects/{}/work-packages/{}",
                segment(project),
                segment(wp)
            ),
            req,
        )
    }

    // Sessions

    pub fn sessions(
        &self,
        project: Option<&str>,
        work_package: Option<&str>,
    ) -> Result<Vec<SessionSummary>> {
        let mut query = Vec::new();
        if let Some(p) = project {
            query.push(format!("project={}", segment(p)));
        }
        if let Some(wp) = work_package {
            query.push(format!("work_package={}", segment(wp)));
        }
        let q = if query.is_empty() {
            String::new()
        } else {
            format!("?{}", query.join("&"))
        };
        self.get(&format!("/sessions{q}"))
    }

    pub fn open_session(&self, req: &OpenSession) -> Result<OpenSessionResult> {
        self.send("POST", "/sessions", req)
    }

    pub fn state(&self, id: &str, binding: Option<&str>) -> Result<SessionState> {
        let q = binding
            .map(|b| format!("?binding={}", segment(b)))
            .unwrap_or_default();
        self.get(&format!("/sessions/{}{q}", segment(id)))
    }

    pub fn document(&self, id: &str) -> Result<Document> {
        self.get(&format!("/sessions/{}/document", segment(id)))
    }

    pub fn update_document(&self, id: &str, req: &UpdateDocument) -> Result<UpdateResult> {
        self.send("PUT", &format!("/sessions/{}/document", segment(id)), req)
    }

    pub fn post_message(&self, id: &str, req: &PostMessage) -> Result<ThreadItem> {
        self.send("POST", &format!("/sessions/{}/messages", segment(id)), req)
    }

    pub fn save_draft(&self, id: &str, decision: &str, req: &SaveDraft) -> Result<Decision> {
        self.send(
            "PUT",
            &format!("/sessions/{}/responses/{}", segment(id), segment(decision)),
            req,
        )
    }

    pub fn confirm(&self, id: &str, decision: &str) -> Result<Decision> {
        self.send(
            "POST",
            &format!(
                "/sessions/{}/responses/{}/confirm",
                segment(id),
                segment(decision)
            ),
            &serde_json::json!({}),
        )
    }

    pub fn record_answer(&self, id: &str, decision: &str, req: &RecordAnswer) -> Result<Decision> {
        self.send(
            "POST",
            &format!(
                "/sessions/{}/responses/{}/record",
                segment(id),
                segment(decision)
            ),
            req,
        )
    }

    pub fn send_drafts(&self, id: &str) -> Result<SendResult> {
        self.send(
            "POST",
            &format!("/sessions/{}/send", segment(id)),
            &serde_json::json!({}),
        )
    }

    pub fn mark_ready(&self, id: &str, req: &MarkReady) -> Result<SessionSummary> {
        self.send("POST", &format!("/sessions/{}/ready", segment(id)), req)
    }

    pub fn accept(&self, id: &str) -> Result<SessionSummary> {
        self.send(
            "POST",
            &format!("/sessions/{}/accept", segment(id)),
            &serde_json::json!({}),
        )
    }

    /// Long poll for the next human turn. `timeout` is the server-side wait.
    pub fn wait(
        &self,
        id: &str,
        binding: Option<&str>,
        since: Option<u64>,
        timeout: Duration,
    ) -> Result<WaitResult> {
        let mut q = format!("?timeout_secs={}", timeout.as_secs().max(1));
        if let Some(b) = binding {
            q.push_str(&format!("&binding={}", segment(b)));
        }
        if let Some(s) = since {
            q.push_str(&format!("&since={s}"));
        }
        self.get_with_timeout(
            &format!("/sessions/{}/wait{q}", segment(id)),
            Some(timeout + Duration::from_secs(30)),
        )
    }

    pub fn attach(&self, id: &str, req: &AttachAgent) -> Result<Binding> {
        self.send("POST", &format!("/sessions/{}/bindings", segment(id)), req)
    }

    pub fn detach(&self, id: &str, binding: &str) -> Result<Binding> {
        self.send(
            "POST",
            &format!(
                "/sessions/{}/bindings/{}/detach",
                segment(id),
                segment(binding)
            ),
            &serde_json::json!({}),
        )
    }

    pub fn handoff(&self, id: &str) -> Result<Handoff> {
        self.get(&format!("/sessions/{}/handoff", segment(id)))
    }

    pub fn close(&self, id: &str, force: bool) -> Result<Record> {
        self.send(
            "POST",
            &format!("/sessions/{}/close", segment(id)),
            &CloseSession { force },
        )
    }

    pub fn record(&self, id: &str) -> Result<Record> {
        self.get(&format!("/sessions/{}/record", segment(id)))
    }

    pub fn attachments(&self, id: &str) -> Result<Vec<Attachment>> {
        self.get(&format!("/sessions/{}/attachments", segment(id)))
    }

    /// Upload an attachment: straight to the server's blob store where it takes uploads
    /// (S3 in the cloud), through the API otherwise.
    pub fn put_attachment(
        &self,
        id: &str,
        path: &str,
        bytes: &[u8],
        content_type: &str,
    ) -> Result<()> {
        let base = format!("/sessions/{}/attachments/{}", segment(id), segment(path));
        let ticket: std::result::Result<UploadTicket, _> = self.send(
            "POST",
            &format!("{base}/upload"),
            &UploadRequest {
                size: bytes.len() as u64,
                content_type: content_type.to_owned(),
            },
        );
        match ticket {
            Ok(UploadTicket {
                url: Some(url),
                headers,
            }) => {
                let mut req = self.agent.put(&url);
                for h in &headers {
                    req = req.header(&h.name, &h.value);
                }
                let mut response = req.send(bytes).map_err(|e| self.unreachable(e))?;
                let status = response.status().as_u16();
                if !(200..300).contains(&status) {
                    let body = response.body_mut().read_to_string().unwrap_or_default();
                    return Err(ClientError::Api {
                        status,
                        code: "upload_failed".into(),
                        message: format!(
                            "the blob store refused the upload (HTTP {status}): {}",
                            body.trim()
                        ),
                    });
                }
                let _: Attachment =
                    self.send("POST", &format!("{base}/complete"), &serde_json::json!({}))?;
                Ok(())
            }
            // No direct upload, or a server from before upload tickets.
            Ok(UploadTicket { url: None, .. })
            | Err(ClientError::Api {
                status: 404 | 405, ..
            }) => {
                let req = self
                    .decorate(self.agent.put(self.url(&base)))
                    .header("content-type", content_type);
                let response = req.send(bytes).map_err(|e| self.unreachable(e))?;
                let status = response.status().as_u16();
                if status == 204 {
                    return Ok(());
                }
                self.finish::<serde_json::Value>(Ok(response)).map(|_| ())
            }
            Err(e) => Err(e),
        }
    }
}

/// A content type for an attachment, from its extension.
pub fn content_type_for(path: &str) -> &'static str {
    let ext = path
        .rsplit_once('.')
        .map(|(_, e)| e.to_ascii_lowercase())
        .unwrap_or_default();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "md" | "markdown" => "text/markdown; charset=utf-8",
        "txt" | "log" => "text/plain; charset=utf-8",
        "json" => "application/json",
        "html" | "htm" => "text/plain; charset=utf-8",
        "csv" => "text/csv; charset=utf-8",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        _ => "application/octet-stream",
    }
}

#[cfg(test)]
mod tests {
    use super::may_carry_token;

    #[test]
    fn tokens_go_over_https_or_to_this_machine() {
        for ok in [
            "https://rt.example.com",
            "http://127.0.0.1:4747",
            "http://localhost:4747/",
            "http://[::1]:4747",
        ] {
            assert!(may_carry_token(ok), "{ok}");
        }
        for bad in [
            "http://rt.example.com",
            "http://127.0.0.1.evil.example",
            "http://localhost@evil.example",
            "http://10.0.0.5:4747",
            "ftp://rt.example.com",
        ] {
            assert!(!may_carry_token(bad), "{bad}");
        }
    }
}
