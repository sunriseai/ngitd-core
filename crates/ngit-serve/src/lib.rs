use ngit_core::{
    accept, add_evidence_from_file, capture, doctor, history, lineage, list_annotations,
    list_drafts, reject, run_evidence, show_record, status, watch_once, CaptureOptions, CoreResult,
};
use serde::Serialize;
use std::{
    error::Error,
    fmt::{Display, Formatter},
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    path::{Component, Path, PathBuf},
};

#[derive(Debug)]
pub enum ServeError {
    Io(std::io::Error),
    Core(ngit_core::CoreError),
    Json(serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct ServeOptions {
    pub bind: String,
    pub once: bool,
    pub token: Option<String>,
    pub allow_non_loopback: bool,
    pub require_auth_for_read: bool,
}

impl ServeOptions {
    pub fn new(bind: String, once: bool) -> Self {
        Self {
            bind,
            once,
            token: None,
            allow_non_loopback: false,
            require_auth_for_read: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RouteContext {
    pub token: Option<String>,
    pub require_auth_for_read: bool,
}

impl Display for ServeError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Core(err) => write!(f, "core error: {err}"),
            Self::Json(err) => write!(f, "json error: {err}"),
        }
    }
}

impl Error for ServeError {}

impl From<std::io::Error> for ServeError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ngit_core::CoreError> for ServeError {
    fn from(value: ngit_core::CoreError) -> Self {
        Self::Core(value)
    }
}

impl From<serde_json::Error> for ServeError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub fn serve(repo_path: &Path, bind: &str, once: bool) -> Result<(), ServeError> {
    serve_with_options(repo_path, ServeOptions::new(bind.to_string(), once))
}

pub fn serve_with_options(repo_path: &Path, mut options: ServeOptions) -> Result<(), ServeError> {
    validate_bind(&options.bind, options.allow_non_loopback)?;
    if options.token.is_none() {
        options.token = Some(format!("ngit-{}", std::process::id()));
    }
    let listener = TcpListener::bind(&options.bind)?;
    eprintln!("ngit serve listening on {}", listener.local_addr()?);
    if let Some(token) = &options.token {
        eprintln!("ngit serve mutation token: {token}");
    }
    let context = RouteContext {
        token: options.token.clone(),
        require_auth_for_read: options.require_auth_for_read,
    };
    for stream in listener.incoming() {
        handle_stream(repo_path, stream?, &context)?;
        if options.once {
            break;
        }
    }
    Ok(())
}

pub fn route(repo_path: &Path, method: &str, path: &str) -> (u16, String) {
    route_with_body(repo_path, method, path, "")
}

pub fn route_with_body(repo_path: &Path, method: &str, path: &str, body: &str) -> (u16, String) {
    route_with_request(
        repo_path,
        method,
        path,
        body,
        None,
        &RouteContext {
            token: None,
            require_auth_for_read: false,
        },
    )
}

pub fn route_with_auth(
    repo_path: &Path,
    method: &str,
    path: &str,
    body: &str,
    authorization: Option<&str>,
    context: &RouteContext,
) -> (u16, String) {
    route_with_request(repo_path, method, path, body, authorization, context)
}

fn route_with_request(
    repo_path: &Path,
    method: &str,
    path: &str,
    body: &str,
    authorization: Option<&str>,
    context: &RouteContext,
) -> (u16, String) {
    if let Err(status) = authorize(method, authorization, context) {
        return json_response(status, &serde_json::json!({ "error": http_reason(status) }));
    }
    let result = match method {
        "GET" => route_get(repo_path, path),
        "POST" => route_post(repo_path, path, body),
        _ => return json_response(405, &serde_json::json!({ "error": "method not allowed" })),
    };
    match result {
        Ok(body) => (200, body),
        Err(err) => json_response(
            error_status(&err),
            &serde_json::json!({ "error": err.to_string() }),
        ),
    }
}

fn route_get(repo_path: &Path, path: &str) -> CoreResult<String> {
    match path {
        "/" | "/status" => json_body(&status(repo_path)?),
        "/drafts" => json_body(&list_drafts(repo_path)?),
        "/history" => json_body(&history(repo_path)?),
        "/doctor" => json_body(&doctor(repo_path)?),
        "/watch" => json_body(&watch_once(repo_path)?),
        other if other.starts_with("/records/") => json_body(&show_record(
            repo_path,
            trim_route_prefix(other, "/records/"),
        )?),
        other if other.starts_with("/lineage/") => {
            json_body(&lineage(repo_path, trim_route_prefix(other, "/lineage/"))?)
        }
        other if other.starts_with("/annotations/") => json_body(&list_annotations(
            repo_path,
            trim_route_prefix(other, "/annotations/"),
        )?),
        other if other.starts_with("/artifacts/") => artifact_body(repo_path, other),
        _ => Err(ngit_core::CoreError::MissingRecord(path.to_string())),
    }
}

fn route_post(repo_path: &Path, path: &str, body: &str) -> CoreResult<String> {
    match path {
        "/capture" => {
            let payload = parse_json_body(body)?;
            let trigger = payload
                .get("trigger")
                .and_then(|value| value.as_str())
                .unwrap_or("api")
                .to_string();
            let intent = payload
                .get("intent")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
            json_body(&capture(repo_path, CaptureOptions { trigger, intent })?)
        }
        other if other.starts_with("/accept/") => {
            let payload = parse_json_body(body)?;
            let override_reason = payload
                .get("override_reason")
                .and_then(|value| value.as_str())
                .map(ToOwned::to_owned);
            json_body(&accept(
                repo_path,
                trim_route_prefix(other, "/accept/"),
                override_reason,
            )?)
        }
        other if other.starts_with("/reject/") => {
            let payload = parse_json_body(body)?;
            let reason = payload
                .get("reason")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string();
            json_body(&reject(
                repo_path,
                trim_route_prefix(other, "/reject/"),
                reason,
            )?)
        }
        other if other.starts_with("/evidence/add/") => {
            let payload = parse_json_body(body)?;
            let file = payload
                .get("file")
                .and_then(|value| value.as_str())
                .unwrap_or("");
            let file = safe_repo_path(repo_path, file)?;
            json_body(&add_evidence_from_file(
                repo_path,
                trim_route_prefix(other, "/evidence/add/"),
                &file,
            )?)
        }
        other if other.starts_with("/evidence/run/") => {
            let payload = parse_json_body(body)?;
            let command = payload
                .get("command")
                .and_then(|value| value.as_array())
                .ok_or_else(|| {
                    ngit_core::CoreError::InvalidInput("missing command array".to_string())
                })?
                .iter()
                .map(|value| value.as_str().unwrap_or("").to_string())
                .collect::<Vec<_>>();
            json_body(&run_evidence(
                repo_path,
                trim_route_prefix(other, "/evidence/run/"),
                &command,
            )?)
        }
        _ => Err(ngit_core::CoreError::MissingRecord(path.to_string())),
    }
}

fn handle_stream(
    repo_path: &Path,
    mut stream: TcpStream,
    context: &RouteContext,
) -> Result<(), ServeError> {
    let mut buffer = [0u8; 8192];
    let n = stream.read(&mut buffer)?;
    let mut request_bytes = buffer[..n].to_vec();
    let header_end = request_bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|idx| idx + 4)
        .unwrap_or(n);
    let headers = String::from_utf8_lossy(&request_bytes[..header_end]);
    let content_length = content_length(&headers).unwrap_or(0);
    let total_needed = header_end.saturating_add(content_length);
    while request_bytes.len() < total_needed {
        let read = stream.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        request_bytes.extend_from_slice(&buffer[..read]);
    }
    let request = String::from_utf8_lossy(&request_bytes);
    let first = request.lines().next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("GET");
    let path = parts.next().unwrap_or("/");
    let authorization = header_value(&request, "authorization");
    let body_start = request.find("\r\n\r\n").map(|idx| idx + 4).unwrap_or(n);
    let request_body = request.get(body_start..).unwrap_or_default();
    let (status, body) = route_with_request(
        repo_path,
        method,
        path,
        request_body,
        authorization.as_deref(),
        context,
    );
    let reason = http_reason(status);
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    )?;
    Ok(())
}

fn json_body<T: Serialize>(value: &T) -> CoreResult<String> {
    serde_json::to_string_pretty(value).map_err(|source| ngit_core::CoreError::MalformedJson {
        path: PathBuf::from("<serve>"),
        source,
    })
}

fn parse_json_body(body: &str) -> CoreResult<serde_json::Value> {
    if body.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(body).map_err(|source| ngit_core::CoreError::MalformedJson {
        path: PathBuf::from("<request-body>"),
        source,
    })
}

fn json_response<T: Serialize>(status: u16, value: &T) -> (u16, String) {
    (
        status,
        serde_json::to_string_pretty(value).unwrap_or_else(|_| "{}".to_string()),
    )
}

fn artifact_body(repo_path: &Path, path: &str) -> CoreResult<String> {
    let relative = percent_decode(trim_route_prefix(path, "/artifacts/"))?;
    let artifact_root = repo_path.join(".ngit/evidence/artifacts");
    let artifact_path = safe_child_path(&artifact_root, &relative)?;
    let text =
        std::fs::read_to_string(&artifact_path).map_err(|source| ngit_core::CoreError::Io {
            path: artifact_path,
            source,
        })?;
    Ok(serde_json::json!({ "artifact": relative, "body": text }).to_string())
}

fn authorize(method: &str, authorization: Option<&str>, context: &RouteContext) -> Result<(), u16> {
    let needs_auth = method != "GET" || context.require_auth_for_read;
    if !needs_auth {
        return Ok(());
    }
    let Some(expected) = &context.token else {
        return Ok(());
    };
    let Some(header) = authorization else {
        return Err(401);
    };
    let token = header.trim().strip_prefix("Bearer ").ok_or(401u16)?;
    if token == expected {
        Ok(())
    } else {
        Err(403)
    }
}

fn validate_bind(bind: &str, allow_non_loopback: bool) -> Result<(), ServeError> {
    if allow_non_loopback {
        return Ok(());
    }
    let addr: SocketAddr = bind.parse().map_err(|_| {
        ServeError::Core(ngit_core::CoreError::InvalidInput(format!(
            "bind address must be an IP socket address: {bind}"
        )))
    })?;
    if addr.ip().is_loopback() {
        Ok(())
    } else {
        Err(ServeError::Core(ngit_core::CoreError::InvalidInput(
            "non-loopback bind requires --allow-non-loopback".to_string(),
        )))
    }
}

fn safe_repo_path(repo_path: &Path, value: &str) -> CoreResult<PathBuf> {
    safe_child_path(repo_path, &percent_decode(value)?)
}

fn safe_child_path(base: &Path, value: &str) -> CoreResult<PathBuf> {
    let relative = Path::new(value);
    if relative.is_absolute()
        || relative.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err(ngit_core::CoreError::InvalidInput(format!(
            "path escapes allowed root: {value}"
        )));
    }
    let base = base
        .canonicalize()
        .map_err(|source| ngit_core::CoreError::Io {
            path: base.to_path_buf(),
            source,
        })?;
    let candidate = base.join(relative);
    let canonical = candidate
        .canonicalize()
        .map_err(|source| ngit_core::CoreError::Io {
            path: candidate.clone(),
            source,
        })?;
    if canonical.starts_with(&base) {
        Ok(canonical)
    } else {
        Err(ngit_core::CoreError::InvalidInput(format!(
            "path escapes allowed root: {value}"
        )))
    }
}

fn percent_decode(value: &str) -> CoreResult<String> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes[idx] == b'%' {
            if idx + 2 >= bytes.len() {
                return Err(ngit_core::CoreError::InvalidInput(
                    "invalid percent encoding".to_string(),
                ));
            }
            let hex = std::str::from_utf8(&bytes[idx + 1..idx + 3]).map_err(|_| {
                ngit_core::CoreError::InvalidInput("invalid percent encoding".to_string())
            })?;
            let byte = u8::from_str_radix(hex, 16).map_err(|_| {
                ngit_core::CoreError::InvalidInput("invalid percent encoding".to_string())
            })?;
            out.push(byte);
            idx += 3;
        } else {
            out.push(bytes[idx]);
            idx += 1;
        }
    }
    String::from_utf8(out)
        .map_err(|_| ngit_core::CoreError::InvalidInput("decoded path is not UTF-8".to_string()))
}

fn content_length(headers: &str) -> Option<usize> {
    header_value(headers, "content-length")?.parse().ok()
}

fn header_value(headers: &str, name: &str) -> Option<String> {
    headers.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        if key.eq_ignore_ascii_case(name) {
            Some(value.trim().to_string())
        } else {
            None
        }
    })
}

fn error_status(err: &ngit_core::CoreError) -> u16 {
    match err {
        ngit_core::CoreError::MissingRecord(_)
        | ngit_core::CoreError::MissingDraft(_)
        | ngit_core::CoreError::NotGitRepo(_)
        | ngit_core::CoreError::NotInitialized(_) => 404,
        ngit_core::CoreError::InvalidInput(_)
        | ngit_core::CoreError::MalformedJson { .. }
        | ngit_core::CoreError::UnsupportedSchema { .. } => 400,
        ngit_core::CoreError::LockHeld(_) | ngit_core::CoreError::DirtyStateChanged(_) => 409,
        _ => 500,
    }
}

fn http_reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        409 => "Conflict",
        _ => "Internal Server Error",
    }
}

fn trim_route_prefix<'a>(path: &'a str, prefix: &str) -> &'a str {
    path.trim_start_matches(prefix)
        .split('?')
        .next()
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ngit_core::init;
    use std::{fs, process::Command};
    use tempfile::TempDir;

    fn git_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        run_ok(&dir, "git", &["init"]);
        run_ok(&dir, "git", &["config", "user.email", "test@example.com"]);
        run_ok(&dir, "git", &["config", "user.name", "Test User"]);
        fs::write(dir.path().join("README.md"), "hello\n").unwrap();
        run_ok(&dir, "git", &["add", "README.md"]);
        run_ok(&dir, "git", &["commit", "-m", "initial"]);
        dir
    }

    fn run_ok(dir: &TempDir, cmd: &str, args: &[&str]) {
        let output = Command::new(cmd)
            .args(args)
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(output.status.success());
    }

    fn context() -> RouteContext {
        RouteContext {
            token: Some("test-token".to_string()),
            require_auth_for_read: false,
        }
    }

    #[test]
    fn routes_status_and_records() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        fs::write(repo.path().join("README.md"), "changed\n").unwrap();
        let draft = capture(
            repo.path(),
            CaptureOptions {
                trigger: "manual".to_string(),
                intent: Some("test intent".to_string()),
            },
        )
        .unwrap();
        let (status_code, body) = route(repo.path(), "GET", "/status");
        assert_eq!(status_code, 200);
        assert!(body.contains("open_drafts"));
        let (status_code, body) =
            route(repo.path(), "GET", &format!("/records/{}", draft.draft_id));
        assert_eq!(status_code, 200);
        assert!(body.contains("Draft"));
    }

    #[test]
    fn post_routes_mutate_through_core() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        fs::write(repo.path().join("README.md"), "changed\n").unwrap();
        let (status_code, body) = route_with_body(
            repo.path(),
            "POST",
            "/capture",
            r#"{"intent":"api intent"}"#,
        );
        assert_eq!(status_code, 200);
        let draft: serde_json::Value = serde_json::from_str(&body).unwrap();
        let draft_id = draft["draft_id"].as_str().unwrap();
        let (status_code, evidence) = route_with_body(
            repo.path(),
            "POST",
            &format!("/evidence/run/{draft_id}"),
            r#"{"command":["rustc","--version"]}"#,
        );
        assert_eq!(status_code, 200);
        assert!(evidence.contains("passed"));
        let (status_code, annotations) =
            route(repo.path(), "GET", &format!("/annotations/{draft_id}"));
        assert_eq!(status_code, 200);
        assert!(annotations.contains("api intent"));
        let (status_code, body) =
            route_with_body(repo.path(), "POST", &format!("/accept/{draft_id}"), "{}");
        assert_eq!(status_code, 200);
        assert!(body.contains("accepted"));
    }

    #[test]
    fn secure_routes_require_auth_for_mutations() {
        let repo = git_repo();
        init(repo.path()).unwrap();
        fs::write(repo.path().join("README.md"), "changed\n").unwrap();
        let ctx = context();

        let (status_code, _) = route_with_auth(repo.path(), "POST", "/capture", "{}", None, &ctx);
        assert_eq!(status_code, 401);

        let (status_code, _) = route_with_auth(
            repo.path(),
            "POST",
            "/capture",
            "{}",
            Some("Bearer wrong"),
            &ctx,
        );
        assert_eq!(status_code, 403);

        let (status_code, body) = route_with_auth(
            repo.path(),
            "POST",
            "/capture",
            r#"{"intent":"secure intent"}"#,
            Some("Bearer test-token"),
            &ctx,
        );
        assert_eq!(status_code, 200);
        let draft: serde_json::Value = serde_json::from_str(&body).unwrap();
        let draft_id = draft["draft_id"].as_str().unwrap();
        let (status_code, body) = route(repo.path(), "GET", &format!("/annotations/{draft_id}"));
        assert_eq!(status_code, 200);
        assert!(body.contains("secure intent"));
    }

    #[test]
    fn routes_return_status_codes_and_reject_path_escape() {
        let repo = git_repo();
        init(repo.path()).unwrap();

        let (status_code, _) = route(repo.path(), "GET", "/missing");
        assert_eq!(status_code, 404);

        let ctx = context();
        let (status_code, _) = route_with_auth(
            repo.path(),
            "POST",
            "/capture",
            "{bad-json",
            Some("Bearer test-token"),
            &ctx,
        );
        assert_eq!(status_code, 400);

        let (status_code, _) = route(repo.path(), "GET", "/artifacts/%2e%2e/manifest.json");
        assert_eq!(status_code, 400);
    }

    #[test]
    fn non_loopback_bind_requires_explicit_opt_in() {
        let options = ServeOptions {
            bind: "0.0.0.0:7878".to_string(),
            once: true,
            token: Some("token".to_string()),
            allow_non_loopback: false,
            require_auth_for_read: false,
        };
        assert!(validate_bind(&options.bind, options.allow_non_loopback).is_err());
        assert!(validate_bind(&options.bind, true).is_ok());
    }
}
