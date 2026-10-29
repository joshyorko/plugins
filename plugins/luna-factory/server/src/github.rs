//! Opt-in, read-only GitHub App reader (#76 intake, #78 delivery telemetry).
//!
//! Everything returned here is labelled `reported_by: "github"`. It is display
//! data only: it never satisfies a criterion, never changes presentation result,
//! actions or attention, and never writes to the Factory ledger. The app private
//! key, app JWTs and installation tokens stay inside this module. They are never
//! serialized, logged, projected or returned in an error string.
use anyhow::{Result, bail, ensure};
use reqwest::{Method, StatusCode, Url, header::HeaderMap};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fmt,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

pub const DEFAULT_API_BASE: &str = "https://api.github.com";
pub const DELIVERY_MIN_INTERVAL_SECONDS: u64 = 60;
const API_VERSION: &str = "2022-11-28";
const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
const MAX_KEY_BYTES: u64 = 16 * 1024;
const MAX_ALLOWED_REPOSITORIES: usize = 32;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const CALL_DEADLINE: Duration = Duration::from_secs(30);
const INTAKE_REQUEST_BUDGET: u32 = 64;
const DELIVERY_REQUEST_BUDGET: u32 = 24;
const DELIVERY_FETCHES_PER_CALL: usize = 16;
const SUB_ISSUES_PER_PAGE: u32 = 25;
const MAX_SUB_ISSUE_PAGES: u32 = 2;
const DEPENDENCIES_PER_PAGE: u32 = 25;
const MAX_TASK_LIST_FETCHES: usize = 16;
const MAX_CANDIDATE_NODES: usize = 32;
const MAX_ACCEPTANCE_PER_ISSUE: usize = 8;
const MAX_ACCEPTANCE_TOTAL: usize = 64;
const MAX_BODY_BYTES: usize = 65536;
const MAX_REPORTED_ITEMS: usize = 64;
const MAX_PULL_REQUESTS: usize = 5;
const MAX_CHECK_CONTEXTS: usize = 25;
const MAX_DELIVERY_CACHE: usize = 512;
const INSTALLATION_TTL: Duration = Duration::from_secs(3600);
const TOKEN_REFRESH_MARGIN_SECONDS: u64 = 300;
const MAX_RATE_LIMIT_WAIT_SECONDS: u64 = 3600;

/// Operator-only configuration. Deliberately not `Serialize`; `Debug` is redacted.
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GithubAppConfig {
    pub app_id: u64,
    #[serde(default)]
    pub installation_id: Option<u64>,
    pub private_key_path: PathBuf,
    #[serde(default = "default_api_base")]
    pub api_base: String,
    pub allowed_repositories: Vec<String>,
}
fn default_api_base() -> String {
    DEFAULT_API_BASE.into()
}
impl fmt::Debug for GithubAppConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GithubAppConfig")
            .field("app_id", &self.app_id)
            .field("installation_id", &self.installation_id)
            .field("private_key_path", &"<redacted>")
            .field("api_base", &self.api_base)
            .field("allowed_repositories", &self.allowed_repositories)
            .finish()
    }
}
impl GithubAppConfig {
    /// Shape validation at configuration load. Key-file readability and
    /// permissions are checked at use and reported as an unavailable reason.
    pub fn validate(&self) -> Result<()> {
        const SAFE_INTEGER: u64 = 9_007_199_254_740_991;
        ensure!(
            (1..=SAFE_INTEGER).contains(&self.app_id),
            "invalid_github_app_id"
        );
        ensure!(
            self.installation_id
                .is_none_or(|id| (1..=SAFE_INTEGER).contains(&id)),
            "invalid_github_installation_id"
        );
        let path = &self.private_key_path;
        ensure!(
            path.is_absolute()
                && path.as_os_str().len() <= 4096
                && path.file_name().is_some()
                && path.components().all(|c| {
                    !matches!(
                        c,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                }),
            "invalid_github_app_key_path"
        );
        ApiBase::parse(&self.api_base)?;
        ensure!(
            !self.allowed_repositories.is_empty()
                && self.allowed_repositories.len() <= MAX_ALLOWED_REPOSITORIES,
            "invalid_github_allowed_repositories"
        );
        let mut seen = BTreeSet::new();
        for entry in &self.allowed_repositories {
            ensure!(
                seen.insert(AllowEntry::parse(entry)?.key()),
                "duplicate_github_allowed_repository"
            );
        }
        Ok(())
    }
    /// An exact `owner/repo` entry keeps its configured spelling; an `owner/*`
    /// entry admits any repository of that owner the installation can see.
    fn allowed(&self, repository: &RepoName) -> Option<RepoName> {
        let entries: Vec<AllowEntry> = self
            .allowed_repositories
            .iter()
            .filter_map(|r| AllowEntry::parse(r).ok())
            .collect();
        entries
            .iter()
            .find_map(|entry| match entry {
                AllowEntry::Repository(name) if name.key() == repository.key() => {
                    Some(name.clone())
                }
                _ => None,
            })
            .or_else(|| {
                entries.iter().find_map(|entry| match entry {
                    AllowEntry::Owner(owner) if owner.eq_ignore_ascii_case(&repository.owner) => {
                        Some(repository.clone())
                    }
                    _ => None,
                })
            })
    }
}

/// One `github_app.allowed_repositories` entry: `owner/repo` or `owner/*`.
#[derive(Debug, Clone, PartialEq, Eq)]
enum AllowEntry {
    Repository(RepoName),
    Owner(String),
}
impl AllowEntry {
    fn parse(value: &str) -> Result<Self> {
        if let Some(owner) = value.strip_suffix("/*") {
            // Reuse the owner rules of a full name with a placeholder repository.
            let name = RepoName::parse(&format!("{owner}/x"))?;
            return Ok(Self::Owner(name.owner));
        }
        Ok(Self::Repository(RepoName::parse(value)?))
    }
    fn key(&self) -> String {
        match self {
            Self::Repository(name) => name.key(),
            Self::Owner(owner) => format!("{}/*", owner.to_ascii_lowercase()),
        }
    }
}

/// `owner/repo` with GitHub's documented character set. Comparison is case-insensitive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoName {
    pub owner: String,
    pub name: String,
}
impl RepoName {
    pub fn parse(value: &str) -> Result<Self> {
        let Some((owner, name)) = value.split_once('/') else {
            bail!("invalid_github_repository");
        };
        ensure!(
            !owner.is_empty()
                && owner.len() <= 39
                && owner
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
                && !owner.starts_with('-')
                && !name.is_empty()
                && name.len() <= 100
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
                && name != "."
                && name != "..",
            "invalid_github_repository"
        );
        Ok(Self {
            owner: owner.into(),
            name: name.into(),
        })
    }
    pub fn full(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
    pub fn key(&self) -> String {
        self.full().to_ascii_lowercase()
    }
}

#[derive(Debug, Clone)]
struct ApiBase {
    url: Url,
    loopback: bool,
    web_host: String,
}
impl ApiBase {
    /// HTTPS origins only. Release builds accept nothing else. Debug builds, which
    /// run the recorded-fixture tests, additionally accept plain HTTP to a literal
    /// loopback IP with an explicit port; that is never a production endpoint.
    fn parse(value: &str) -> Result<Self> {
        Self::parse_with(value, cfg!(debug_assertions))
    }
    fn parse_with(value: &str, loopback_fixture: bool) -> Result<Self> {
        ensure!(
            value.len() <= 256 && !value.chars().any(|c| c.is_control() || c == ' '),
            "invalid_github_api_base"
        );
        let url = Url::parse(value).map_err(|_| anyhow::anyhow!("invalid_github_api_base"))?;
        let path_ok = url.path() == "/" && !value.ends_with('/');
        ensure!(
            path_ok
                && url.username().is_empty()
                && url.password().is_none()
                && url.query().is_none()
                && url.fragment().is_none(),
            "github_api_base_origin_required"
        );
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        let literal_loopback = host
            .trim_start_matches('[')
            .trim_end_matches(']')
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());
        match url.scheme() {
            "https" => ensure!(!host.is_empty(), "invalid_github_api_base"),
            "http" => ensure!(
                loopback_fixture && literal_loopback && url.port().is_some(),
                "github_api_base_https_required"
            ),
            _ => bail!("github_api_base_https_required"),
        }
        let web_host = if literal_loopback || host == "api.github.com" {
            "github.com".to_owned()
        } else if let Some(rest) = host.strip_prefix("api.") {
            rest.to_owned()
        } else {
            host.clone()
        };
        Ok(Self {
            url,
            loopback: literal_loopback,
            web_host,
        })
    }
    fn endpoint(&self, path: &str) -> std::result::Result<Url, Failure> {
        self.url
            .join(path)
            .map_err(|_| Failure::Unavailable("github_invalid_request"))
    }
    fn issue_url(&self, repository: &RepoName, number: u64) -> String {
        format!(
            "https://{}/{}/issues/{number}",
            self.web_host,
            repository.full()
        )
    }
    fn pull_url(&self, repository: &RepoName, number: u64) -> String {
        format!(
            "https://{}/{}/pull/{number}",
            self.web_host,
            repository.full()
        )
    }
}

/// Never Debug-printed, serialized or formatted into an error.
#[derive(Clone)]
struct Secret(String);
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    Unavailable(&'static str),
    RateLimited { retry_at: u64 },
}
impl Failure {
    fn reason(&self) -> &'static str {
        match self {
            Self::Unavailable(reason) => reason,
            Self::RateLimited { .. } => "rate_limited",
        }
    }
    fn retry_at(&self) -> Option<u64> {
        match self {
            Self::RateLimited { retry_at } => Some(*retry_at),
            Self::Unavailable(_) => None,
        }
    }
    /// Transport-level failures stop the rest of a multi-request call.
    fn is_global(&self) -> bool {
        matches!(
            self,
            Self::RateLimited { .. }
                | Self::Unavailable(
                    "github_unreachable"
                        | "github_timeout"
                        | "github_call_deadline_exceeded"
                        | "github_request_budget_exhausted"
                        | "github_authentication_failed"
                        | "github_app_not_installed"
                        | "github_installation_mismatch"
                        | "github_app_not_read_only"
                        | "github_token_scope_mismatch"
                        | "github_app_key_unreadable"
                        | "github_app_key_permissions_too_open"
                        | "github_app_key_not_regular_file"
                        | "github_app_key_symlinked"
                        | "github_app_key_invalid"
                        | "github_service_unavailable"
                )
        )
    }
}
type Outcome<T> = std::result::Result<T, Failure>;

/// Read scopes requested for each installation token. Repository tokens are
/// always narrowed to one repository and to these read-only permissions. The
/// listing token is the one exception: it must enumerate the installation, so it
/// is not narrowed to a repository and carries `metadata: read` only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Scope {
    Intake,
    Delivery,
    /// Clone one connected repository: `contents: read` only.
    Contents,
    /// `GET /installation/repositories`: `metadata: read` only.
    List,
}
impl Scope {
    /// Permissions requested in the token body, all `read`.
    fn permissions(self) -> &'static [&'static str] {
        match self {
            Self::Intake => &["issues", "metadata"],
            Self::Delivery => &["checks", "issues", "metadata", "pull_requests", "statuses"],
            Self::Contents => &["contents"],
            Self::List => &["metadata"],
        }
    }
    /// Permissions a minted token may report. GitHub always adds `metadata: read`.
    fn accepted(self) -> &'static [&'static str] {
        match self {
            Self::Contents => &["contents", "metadata"],
            _ => self.permissions(),
        }
    }
    fn body(self, repository: Option<&RepoName>) -> Value {
        let permissions: serde_json::Map<String, Value> = self
            .permissions()
            .iter()
            .map(|p| ((*p).to_owned(), json!("read")))
            .collect();
        match repository {
            Some(repository) => {
                json!({"repositories":[repository.name],"permissions":permissions})
            }
            None => json!({"permissions":permissions}),
        }
    }
}

#[derive(Serialize)]
struct AppClaims {
    iat: u64,
    exp: u64,
    iss: String,
}

/// Placement checks shared by the capability report and by every key read.
/// The key must be a regular file reached without any symlink (neither the file
/// nor any ancestor directory), readable by its owner only, and bounded in size.
fn check_key_metadata(metadata: &std::fs::Metadata) -> Outcome<()> {
    if metadata.file_type().is_symlink() {
        return Err(Failure::Unavailable("github_app_key_symlinked"));
    }
    if !metadata.file_type().is_file() {
        return Err(Failure::Unavailable("github_app_key_not_regular_file"));
    }
    #[cfg(not(unix))]
    return Err(Failure::Unavailable(
        "github_app_key_permissions_unverifiable",
    ));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(Failure::Unavailable("github_app_key_permissions_too_open"));
        }
    }
    if metadata.len() == 0 || metadata.len() > MAX_KEY_BYTES {
        return Err(Failure::Unavailable("github_app_key_invalid"));
    }
    Ok(())
}
/// Validate operator key placement without reading its contents.
fn key_metadata(path: &Path) -> Outcome<()> {
    let link = std::fs::symlink_metadata(path)
        .map_err(|_| Failure::Unavailable("github_app_key_unreadable"))?;
    check_key_metadata(&link)?;
    // A symlinked ancestor directory is refused too: the configured path must
    // already be canonical, so the operator sees exactly which file is read.
    let canonical = std::fs::canonicalize(path)
        .map_err(|_| Failure::Unavailable("github_app_key_unreadable"))?;
    if canonical != path {
        return Err(Failure::Unavailable("github_app_key_symlinked"));
    }
    Ok(())
}

/// GitHub generates PKCS#1 PEM (`BEGIN RSA PRIVATE KEY`). The key is reread for
/// every JWT so rotation needs no restart, and it is never cached or logged.
/// The file is opened without following a final symlink and re-checked through
/// the opened handle, so a swap between the check and the read is refused.
fn signing_key(path: &Path) -> Outcome<jsonwebtoken::EncodingKey> {
    use base64::Engine;
    use std::io::Read;
    key_metadata(path)?;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let file = options.open(path).map_err(|error| {
        Failure::Unavailable(if cfg!(unix) && error.raw_os_error() == Some(libc::ELOOP) {
            "github_app_key_symlinked"
        } else {
            "github_app_key_unreadable"
        })
    })?;
    let opened = file
        .metadata()
        .map_err(|_| Failure::Unavailable("github_app_key_unreadable"))?;
    check_key_metadata(&opened)?;
    let mut bytes = Vec::new();
    file.take(MAX_KEY_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Failure::Unavailable("github_app_key_unreadable"))?;
    if bytes.len() as u64 > MAX_KEY_BYTES {
        return Err(Failure::Unavailable("github_app_key_invalid"));
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| Failure::Unavailable("github_app_key_invalid"))?;
    let begin = "-----BEGIN RSA PRIVATE KEY-----";
    let end = "-----END RSA PRIVATE KEY-----";
    let start = text
        .find(begin)
        .ok_or(Failure::Unavailable("github_app_key_format_unsupported"))?;
    let rest = &text[start + begin.len()..];
    let stop = rest
        .find(end)
        .ok_or(Failure::Unavailable("github_app_key_invalid"))?;
    let encoded: String = rest[..stop]
        .chars()
        .filter(|c| !c.is_ascii_whitespace())
        .collect();
    if encoded.contains(':') {
        // Encrypted legacy PEM headers are not supported.
        return Err(Failure::Unavailable("github_app_key_format_unsupported"));
    }
    let der = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|_| Failure::Unavailable("github_app_key_invalid"))?;
    Ok(jsonwebtoken::EncodingKey::from_rsa_der(&der))
}

/// RS256 app JWT: issued 60 s in the past and valid for at most ten minutes.
fn app_jwt(config: &GithubAppConfig, now: u64) -> Outcome<Secret> {
    let key = signing_key(&config.private_key_path)?;
    let claims = AppClaims {
        iat: now.saturating_sub(60),
        exp: now + 540,
        iss: config.app_id.to_string(),
    };
    jsonwebtoken::encode(
        &jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256),
        &claims,
        &key,
    )
    .map(Secret)
    .map_err(|_| Failure::Unavailable("github_app_key_invalid"))
}

/// Strict UTC `YYYY-MM-DDTHH:MM:SSZ` (GitHub's format) to Unix seconds.
pub fn parse_timestamp(value: &str) -> Option<u64> {
    let b = value.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return None;
    }
    let digits = |range: std::ops::Range<usize>| -> Option<i64> {
        let part = value.get(range)?;
        part.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| part.parse().ok())?
    };
    let (year, month, day) = (digits(0..4)?, digits(5..7)?, digits(8..10)?);
    let (hour, minute, second) = (digits(11..13)?, digits(14..16)?, digits(17..19)?);
    if !(1970..=9999).contains(&year)
        || !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }
    // Howard Hinnant's days-from-civil.
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hour * 3600 + minute * 60 + second).ok()
}

fn bounded_text(value: &str, limit: usize) -> Option<String> {
    let summary = crate::lifecycle::safe_summary(value.trim(), limit);
    let single: String = summary
        .chars()
        .map(|c| if c == '\n' { ' ' } else { c })
        .collect();
    let trimmed = single.trim();
    (!trimmed.is_empty() && !summary.starts_with("Sensitive details withheld"))
        .then(|| trimmed.to_owned())
}
fn bounded_prefix(value: &str, limit: usize) -> &str {
    if value.len() <= limit {
        return value;
    }
    let mut end = limit;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    &value[..end]
}
/// A conservative branch name: what GitHub default branches use in practice, with
/// nothing git could read as an option, a range or a path escape.
pub fn valid_branch(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && !value.starts_with(['-', '/', '.'])
        && !value.ends_with(['/', '.'])
        && !value.ends_with(".lock")
        && !value.contains("..")
        && !value.contains("//")
        && !value.contains("@{")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'/'))
}
fn valid_node_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'='))
}

struct Reply {
    status: StatusCode,
    body: Value,
    has_next: bool,
    rate_reset: Option<u64>,
}
impl Reply {
    fn ok(self) -> Outcome<Self> {
        if self.status.is_success() {
            return Ok(self);
        }
        Err(Failure::Unavailable(status_reason(self.status)))
    }
}
fn status_reason(status: StatusCode) -> &'static str {
    match status.as_u16() {
        300..=399 => "github_redirect_refused",
        401 => "github_authentication_failed",
        403 => "github_permission_denied",
        404 => "github_not_found",
        410 => "github_gone",
        422 => "github_request_rejected",
        500..=599 => "github_service_unavailable",
        _ => "github_unexpected_status",
    }
}
fn header_u64(headers: &HeaderMap, name: &str) -> Option<u64> {
    headers.get(name)?.to_str().ok()?.trim().parse::<u64>().ok()
}
/// Primary and secondary rate limits become a structured, bounded retry time.
fn rate_limit(status: StatusCode, headers: &HeaderMap, now: u64) -> Option<u64> {
    let remaining = headers
        .get("x-ratelimit-remaining")
        .and_then(|v| v.to_str().ok())
        .map(str::trim);
    let retry_after = header_u64(headers, "retry-after");
    let limited = status == StatusCode::TOO_MANY_REQUESTS
        || (status == StatusCode::FORBIDDEN && (remaining == Some("0") || retry_after.is_some()));
    if !limited {
        return None;
    }
    let cap = now + MAX_RATE_LIMIT_WAIT_SECONDS;
    Some(if let Some(seconds) = retry_after {
        (now + seconds).min(cap)
    } else if let Some(reset) = header_u64(headers, "x-ratelimit-reset") {
        reset.clamp(now + 1, cap)
    } else {
        now + 60
    })
}

struct CallBudget {
    remaining: u32,
    deadline: Instant,
}
impl CallBudget {
    fn new(requests: u32) -> Self {
        Self {
            remaining: requests,
            deadline: Instant::now() + CALL_DEADLINE,
        }
    }
}

#[derive(Clone)]
struct CachedToken {
    token: Secret,
    repository_id: u64,
    repository_node_id: String,
    /// The granted repository's default branch, when GitHub reported a safe name.
    default_branch: Option<String>,
    /// The installation behind this token was granted `contents: read`.
    contents_read: bool,
    refresh_after: u64,
}
/// What Luna needs to know about an installation after the read-only check.
#[derive(Debug, Clone, Copy)]
struct Installation {
    id: u64,
    /// The installation was granted `contents: read` (needed to connect a repository).
    contents_read: bool,
}
#[derive(Clone)]
struct CachedDelivery {
    /// Last attempt, successful or not; it gates the per-node minimum interval.
    attempted: Instant,
    /// Last successful observation, kept for stale display after a failure.
    value: Option<Value>,
    /// Reason the last attempt failed, or `None` when it succeeded.
    failure: Option<&'static str>,
}
#[derive(Default)]
struct State {
    installations: HashMap<String, (Installation, Instant)>,
    tokens: HashMap<(String, Scope), CachedToken>,
    rate_limited_until: Option<u64>,
    delivery: HashMap<String, CachedDelivery>,
}

/// One reader per service. Network IO never runs under the state lock.
pub struct GithubReader {
    config: Option<GithubAppConfig>,
    base: Option<ApiBase>,
    http: Option<reqwest::Client>,
    state: Mutex<State>,
    requests: std::sync::atomic::AtomicU64,
    /// Debug-build fixture only: clone from `<dir>/<owner>/<repo>.git` instead of GitHub.
    clone_source: std::sync::OnceLock<PathBuf>,
    /// Debug-build race seam invoked after token minting and before Git starts.
    clone_hook: std::sync::OnceLock<Arc<dyn Fn() + Send + Sync>>,
    /// Debug-build seam invoked once Git has finished touching the staging tree.
    clone_complete_hook: std::sync::OnceLock<Arc<dyn Fn() + Send + Sync>>,
}
impl fmt::Debug for GithubReader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GithubReader")
            .field("configured", &self.config.is_some())
            .finish_non_exhaustive()
    }
}

/// Local, network-free eligibility: GitHub App allowlist plus a Luna-approved
/// repository whose configured remote names the same `owner/repo`.
#[derive(Debug, Clone, Serialize)]
pub struct Eligibility {
    pub eligible: bool,
    pub allowed_by_github_app: bool,
    pub approved_aliases: Vec<String>,
    pub reasons: Vec<&'static str>,
}

/// Approved aliases whose Git remotes point at `repository` on `web_host`.
pub fn approved_aliases(
    config: &crate::config::Config,
    repository: &RepoName,
    web_host: &str,
) -> Vec<String> {
    config
        .repositories
        .iter()
        .take(32)
        .filter(|(_, repo)| {
            remote_repositories(&repo.root, web_host)
                .iter()
                .any(|r| r.key() == repository.key())
        })
        .map(|(alias, _)| alias.clone())
        .take(8)
        .collect()
}
fn remote_repositories(root: &Path, web_host: &str) -> Vec<RepoName> {
    crate::store::git(root, &["config", "--get-regexp", r"^remote\..*\.url$"])
        .map(|text| {
            text.lines()
                .take(32)
                .filter_map(|line| line.split_once(' ').map(|(_, url)| url.trim()))
                .filter_map(|url| parse_remote(url, web_host))
                .collect()
        })
        .unwrap_or_default()
}
/// Recognise `https://host/owner/repo(.git)`, `ssh://git@host/owner/repo(.git)`
/// and `git@host:owner/repo(.git)`. Anything else is not a match.
pub fn parse_remote(url: &str, web_host: &str) -> Option<RepoName> {
    let host = web_host.to_ascii_lowercase();
    let lower = url.to_ascii_lowercase();
    let path = if let Some(rest) = lower
        .strip_prefix("https://")
        .or_else(|| lower.strip_prefix("ssh://"))
    {
        let rest = rest.rsplit_once('@').map_or(rest, |(_, r)| r);
        let (authority, path) = rest.split_once('/')?;
        let authority = authority.split_once(':').map_or(authority, |(h, _)| h);
        (authority == host).then_some(path)?
    } else {
        let rest = lower.strip_prefix("git@")?;
        let (authority, path) = rest.split_once(':')?;
        (authority == host).then_some(path)?
    };
    let start = lower.len() - path.len();
    let original = &url[start..];
    let original = original.trim_end_matches('/');
    let original = original.strip_suffix(".git").unwrap_or(original);
    RepoName::parse(original).ok()
}

impl GithubReader {
    pub fn new(config: Option<GithubAppConfig>) -> Self {
        let base = config
            .as_ref()
            .and_then(|config| ApiBase::parse(&config.api_base).ok());
        let http = base.as_ref().and_then(|base| {
            let mut builder = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(REQUEST_TIMEOUT)
                .connect_timeout(CONNECT_TIMEOUT)
                .user_agent(concat!("luna-factory/", env!("CARGO_PKG_VERSION")));
            builder = if base.loopback {
                builder.no_proxy()
            } else {
                // A standard HTTPS_PROXY (Option A egress proxy) only sees CONNECT.
                builder.https_only(true)
            };
            builder.build().ok()
        });
        Self {
            config,
            base,
            http,
            state: Mutex::new(State::default()),
            requests: std::sync::atomic::AtomicU64::new(0),
            clone_source: std::sync::OnceLock::new(),
            clone_hook: std::sync::OnceLock::new(),
            clone_complete_hook: std::sync::OnceLock::new(),
        }
    }
    /// Network requests attempted by this reader (diagnostic counter for tests).
    pub fn request_count(&self) -> u64 {
        self.requests.load(std::sync::atomic::Ordering::SeqCst)
    }
    pub fn web_host(&self) -> &str {
        self.base
            .as_ref()
            .map_or("github.com", |base| base.web_host.as_str())
    }
    pub fn allowed(&self, repository: &RepoName) -> Option<RepoName> {
        self.config.as_ref()?.allowed(repository)
    }
    /// Exact allowlist entries. `owner/*` entries name no single repository.
    pub fn allowed_repositories(&self) -> Vec<RepoName> {
        self.config.as_ref().map_or_else(Vec::new, |config| {
            config
                .allowed_repositories
                .iter()
                .filter_map(|r| match AllowEntry::parse(r) {
                    Ok(AllowEntry::Repository(name)) => Some(name),
                    _ => None,
                })
                .collect()
        })
    }
    /// Allowlisted repositories named by the Git remotes of an approved root.
    pub fn approved_remote_repositories(&self, root: &Path) -> Vec<RepoName> {
        let mut out: Vec<RepoName> = Vec::new();
        for remote in remote_repositories(root, self.web_host()) {
            if let Some(allowed) = self.allowed(&remote)
                && !out.iter().any(|r| r.key() == allowed.key())
            {
                out.push(allowed);
            }
        }
        out
    }
    /// Test hook: age cached delivery observations past the minimum interval.
    /// It cannot raise the per-call fetch bound and is not reachable from MCP.
    #[doc(hidden)]
    pub async fn expire_delivery_cache(&self) {
        let interval = Duration::from_secs(DELIVERY_MIN_INTERVAL_SECONDS + 1);
        if let Some(past) = Instant::now().checked_sub(interval) {
            for entry in self.state.lock().await.delivery.values_mut() {
                entry.attempted = past;
            }
        }
    }
    /// Why the integration is unavailable, or `None` when it is configured.
    pub fn unavailable_reason(&self) -> Option<&'static str> {
        self.unconfigured_reason()
    }
    fn unconfigured_reason(&self) -> Option<&'static str> {
        let config = match &self.config {
            None => return Some("github_app_not_configured"),
            Some(config) => config,
        };
        if config.validate().is_err() || self.base.is_none() {
            return Some("github_app_config_invalid");
        }
        if self.http.is_none() {
            return Some("github_client_unavailable");
        }
        key_metadata(&config.private_key_path)
            .err()
            .map(|failure| failure.reason())
    }
    /// `{configured, reason}` only. Reachability is reported per read.
    pub fn capability(&self) -> Value {
        match self.unconfigured_reason() {
            Some(reason) => json!({"configured":false,"reason":reason}),
            None => json!({"configured":true,"reason":"configured_reachability_unverified"}),
        }
    }

    async fn send(
        &self,
        budget: &mut CallBudget,
        method: Method,
        url: Url,
        credential: &Secret,
        body: Option<Value>,
    ) -> Outcome<Reply> {
        let now = crate::store::now();
        if let Some(until) = self.state.lock().await.rate_limited_until
            && now < until
        {
            return Err(Failure::RateLimited { retry_at: until });
        }
        let http = self
            .http
            .as_ref()
            .ok_or(Failure::Unavailable("github_client_unavailable"))?;
        if budget.remaining == 0 {
            return Err(Failure::Unavailable("github_request_budget_exhausted"));
        }
        let left = budget.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(Failure::Unavailable("github_call_deadline_exceeded"));
        }
        budget.remaining -= 1;
        let mut authorization =
            reqwest::header::HeaderValue::from_str(&format!("Bearer {}", credential.0))
                .map_err(|_| Failure::Unavailable("github_authentication_failed"))?;
        authorization.set_sensitive(true);
        let mut request = http
            .request(method, url)
            .timeout(left.min(REQUEST_TIMEOUT))
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .header("X-GitHub-Api-Version", API_VERSION)
            .header(reqwest::header::AUTHORIZATION, authorization);
        if let Some(body) = body {
            request = request.json(&body);
        }
        self.requests
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut response = request.send().await.map_err(|error| {
            Failure::Unavailable(if error.is_timeout() {
                "github_timeout"
            } else {
                "github_unreachable"
            })
        })?;
        let status = response.status();
        let headers = response.headers().clone();
        if let Some(retry_at) = rate_limit(status, &headers, crate::store::now()) {
            self.state.lock().await.rate_limited_until = Some(retry_at);
            return Err(Failure::RateLimited { retry_at });
        }
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(Failure::Unavailable("github_response_too_large"));
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|error| {
            Failure::Unavailable(if error.is_timeout() {
                "github_timeout"
            } else {
                "github_unreachable"
            })
        })? {
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(Failure::Unavailable("github_response_too_large"));
            }
            bytes.extend_from_slice(&chunk);
        }
        let body = if bytes.is_empty() || !status.is_success() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .map_err(|_| Failure::Unavailable("github_invalid_response"))?
        };
        let has_next = headers
            .get_all(reqwest::header::LINK)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .any(|v| v.split(',').any(|part| part.contains("rel=\"next\"")));
        Ok(Reply {
            status,
            body,
            has_next,
            rate_reset: header_u64(&headers, "x-ratelimit-reset"),
        })
    }

    fn base(&self) -> Outcome<&ApiBase> {
        self.base
            .as_ref()
            .ok_or(Failure::Unavailable("github_app_config_invalid"))
    }
    fn config(&self) -> Outcome<&GithubAppConfig> {
        self.config
            .as_ref()
            .ok_or(Failure::Unavailable("github_app_not_configured"))
    }

    async fn cached_installation(&self, key: &str) -> Option<Installation> {
        self.state
            .lock()
            .await
            .installations
            .get(key)
            .copied()
            .filter(|(_, at)| at.elapsed() < INSTALLATION_TTL)
            .map(|(installation, _)| installation)
    }
    /// The pinned-installation and read-only checks shared by every lookup.
    fn checked_installation(&self, body: &Value) -> Outcome<Installation> {
        let config = self.config()?;
        let id = body["id"]
            .as_u64()
            .filter(|id| *id > 0)
            .ok_or(Failure::Unavailable("github_invalid_response"))?;
        if config
            .installation_id
            .is_some_and(|expected| expected != id)
        {
            return Err(Failure::Unavailable("github_installation_mismatch"));
        }
        // Defense in depth: refuse an installation that was granted any write scope.
        // `contents: read` is a read scope and passes.
        let permissions = body["permissions"].as_object();
        if let Some(permissions) = permissions
            && permissions
                .values()
                .any(|level| level.as_str() != Some("read"))
        {
            return Err(Failure::Unavailable("github_app_not_read_only"));
        }
        Ok(Installation {
            id,
            contents_read: permissions
                .and_then(|p| p.get("contents"))
                .and_then(Value::as_str)
                == Some("read"),
        })
    }
    async fn installation(
        &self,
        budget: &mut CallBudget,
        repository: &RepoName,
        jwt: &Secret,
    ) -> Outcome<Installation> {
        if let Some(installation) = self.cached_installation(&repository.key()).await {
            return Ok(installation);
        }
        let url = self.base()?.endpoint(&format!(
            "/repos/{}/{}/installation",
            repository.owner, repository.name
        ))?;
        let reply = self.send(budget, Method::GET, url, jwt, None).await?;
        if reply.status == StatusCode::NOT_FOUND {
            return Err(Failure::Unavailable("github_app_not_installed"));
        }
        let installation = self.checked_installation(&reply.ok()?.body)?;
        self.state
            .lock()
            .await
            .installations
            .insert(repository.key(), (installation, Instant::now()));
        Ok(installation)
    }
    /// The installation whose repositories are listed: the pinned one, or the
    /// app's only installation. More than one unpinned installation is ambiguous.
    async fn listing_installation(
        &self,
        budget: &mut CallBudget,
        jwt: &Secret,
    ) -> Outcome<Installation> {
        const KEY: &str = "*";
        if let Some(installation) = self.cached_installation(KEY).await {
            return Ok(installation);
        }
        let base = self.base()?;
        let body = match self.config()?.installation_id {
            Some(id) => {
                let url = base.endpoint(&format!("/app/installations/{id}"))?;
                let reply = self.send(budget, Method::GET, url, jwt, None).await?;
                if reply.status == StatusCode::NOT_FOUND {
                    return Err(Failure::Unavailable("github_app_not_installed"));
                }
                reply.ok()?.body
            }
            None => {
                let mut url = base.endpoint("/app/installations")?;
                url.query_pairs_mut().append_pair("per_page", "2");
                let body = self
                    .send(budget, Method::GET, url, jwt, None)
                    .await?
                    .ok()?
                    .body;
                match body.as_array().map(Vec::as_slice) {
                    Some([only]) => only.clone(),
                    Some([]) => return Err(Failure::Unavailable("github_app_not_installed")),
                    Some(_) => return Err(Failure::Unavailable("github_installation_ambiguous")),
                    None => return Err(Failure::Unavailable("github_invalid_response")),
                }
            }
        };
        let installation = self.checked_installation(&body)?;
        self.state
            .lock()
            .await
            .installations
            .insert(KEY.to_owned(), (installation, Instant::now()));
        Ok(installation)
    }

    /// A single-repository, read-only installation token, cached in memory per
    /// repository and permission scope until shortly before expiry.
    async fn token(
        &self,
        budget: &mut CallBudget,
        repository: &RepoName,
        scope: Scope,
    ) -> Outcome<CachedToken> {
        let config = self.config()?;
        if config.allowed(repository).is_none() {
            return Err(Failure::Unavailable(
                "repository_not_in_github_app_allowlist",
            ));
        }
        self.mint(budget, Some(repository), scope).await
    }
    /// Mint (or reuse) an installation token. With a repository, the token is
    /// narrowed to it and its grant is checked against it; without one (listing
    /// only), it carries `metadata: read` and no repository binding.
    async fn mint(
        &self,
        budget: &mut CallBudget,
        repository: Option<&RepoName>,
        scope: Scope,
    ) -> Outcome<CachedToken> {
        let config = self.config()?;
        let now = crate::store::now();
        let installation_key = repository.map_or_else(|| "*".to_owned(), RepoName::key);
        let key = (installation_key.clone(), scope);
        if let Some(token) = self.state.lock().await.tokens.get(&key)
            && now < token.refresh_after
        {
            return Ok(token.clone());
        }
        let jwt = app_jwt(config, now)?;
        let installation = match repository {
            Some(repository) => self.installation(budget, repository, &jwt).await?,
            None => self.listing_installation(budget, &jwt).await?,
        };
        if scope == Scope::Contents && !installation.contents_read {
            return Err(Failure::Unavailable("github_contents_permission_missing"));
        }
        let url = self.base()?.endpoint(&format!(
            "/app/installations/{}/access_tokens",
            installation.id
        ))?;
        let reply = self
            .send(
                budget,
                Method::POST,
                url,
                &jwt,
                Some(scope.body(repository)),
            )
            .await?;
        if matches!(reply.status.as_u16(), 401 | 404) {
            self.state
                .lock()
                .await
                .installations
                .remove(&installation_key);
        }
        let body = reply.ok()?.body;
        let token = body["token"]
            .as_str()
            .filter(|t| {
                !t.is_empty()
                    && t.len() <= 512
                    && t.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
            })
            .ok_or(Failure::Unavailable("github_invalid_response"))?;
        let granted = body["permissions"]
            .as_object()
            .ok_or(Failure::Unavailable("github_token_scope_mismatch"))?;
        let accepted = scope.accepted();
        if granted.is_empty()
            || granted.iter().any(|(name, level)| {
                !accepted.contains(&name.as_str()) || level.as_str() != Some("read")
            })
            // A clone token that lacks the one scope it exists for is no use.
            || (scope == Scope::Contents
                && granted.get("contents").and_then(Value::as_str) != Some("read"))
        {
            return Err(Failure::Unavailable("github_token_scope_mismatch"));
        }
        let (repository_id, repository_node_id, default_branch) = match repository {
            None => (0, String::new(), None),
            Some(repository) => {
                let repositories = body["repositories"]
                    .as_array()
                    .ok_or(Failure::Unavailable("github_token_scope_mismatch"))?;
                let [granted_repository] = repositories.as_slice() else {
                    return Err(Failure::Unavailable("github_token_scope_mismatch"));
                };
                if granted_repository["full_name"]
                    .as_str()
                    .map(str::to_ascii_lowercase)
                    != Some(repository.key())
                {
                    return Err(Failure::Unavailable("github_token_scope_mismatch"));
                }
                let repository_id = granted_repository["id"]
                    .as_u64()
                    .filter(|id| *id > 0)
                    .ok_or(Failure::Unavailable("github_invalid_response"))?;
                let repository_node_id = granted_repository["node_id"]
                    .as_str()
                    .filter(|id| valid_node_id(id))
                    .unwrap_or_default()
                    .to_owned();
                let default_branch = granted_repository["default_branch"]
                    .as_str()
                    .filter(|branch| valid_branch(branch))
                    .map(str::to_owned);
                (repository_id, repository_node_id, default_branch)
            }
        };
        let expires_at = body["expires_at"].as_str().and_then(parse_timestamp);
        let cached = CachedToken {
            token: Secret(token.to_owned()),
            repository_id,
            repository_node_id,
            default_branch,
            contents_read: installation.contents_read,
            refresh_after: expires_at
                .map_or(0, |at| at.saturating_sub(TOKEN_REFRESH_MARGIN_SECONDS)),
        };
        if cached.refresh_after > now {
            self.state.lock().await.tokens.insert(key, cached.clone());
        }
        Ok(cached)
    }
    async fn evict_token(&self, repository: &RepoName, scope: Scope) {
        self.state
            .lock()
            .await
            .tokens
            .remove(&(repository.key(), scope));
    }
    async fn get(
        &self,
        budget: &mut CallBudget,
        repository: &RepoName,
        scope: Scope,
        token: &CachedToken,
        path: &str,
        query: &[(&str, String)],
    ) -> Outcome<Reply> {
        let mut url = self.base()?.endpoint(path)?;
        if !query.is_empty() {
            let mut pairs = url.query_pairs_mut();
            for (name, value) in query {
                pairs.append_pair(name, value);
            }
        }
        let reply = self
            .send(budget, Method::GET, url, &token.token, None)
            .await?;
        if reply.status == StatusCode::UNAUTHORIZED {
            self.evict_token(repository, scope).await;
        }
        Ok(reply)
    }
}

// ----------------------------------------------------------------------------
// #76: source-bound issue-graph intake. Reads only; writes nothing.

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InspectIssueGraph {
    pub repository: String,
    pub parent: u64,
    /// Approved Luna alias to bind sources to. Required only when more than one
    /// approved repository's remote names the same GitHub repository.
    #[serde(default)]
    pub repository_alias: Option<String>,
}

#[derive(Debug, Clone)]
struct Issue {
    node_id: String,
    number: u64,
    title: String,
    body: String,
    open: bool,
    repository: Option<RepoName>,
    pull_request: bool,
    blocked_by_total: Option<u64>,
    /// GitHub's own sub-issue count, when reported, to detect short sub-issue pages.
    sub_issues_total: Option<u64>,
    /// The body exceeded the bound and was cut, so prose parsed from it may be incomplete.
    body_truncated: bool,
}
fn repository_from_api_url(value: &str) -> Option<RepoName> {
    let (_, tail) = value.rsplit_once("/repos/")?;
    let mut parts = tail.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    RepoName::parse(&format!("{owner}/{name}")).ok()
}
fn issue(value: &Value) -> Option<Issue> {
    let node_id = value["node_id"].as_str().filter(|id| valid_node_id(id))?;
    let number = value["number"].as_u64().filter(|n| *n > 0)?;
    let title = value["title"].as_str()?;
    let state = value["state"].as_str()?;
    let summary = &value["issue_dependencies_summary"];
    let body = value["body"].as_str().unwrap_or_default();
    Some(Issue {
        node_id: node_id.into(),
        number,
        title: bounded_prefix(title, 1024).into(),
        body: bounded_prefix(body, MAX_BODY_BYTES).into(),
        body_truncated: body.len() > MAX_BODY_BYTES,
        open: state == "open",
        repository: value["repository_url"]
            .as_str()
            .and_then(repository_from_api_url),
        pull_request: value.get("pull_request").is_some_and(|v| !v.is_null()),
        blocked_by_total: summary["total_blocked_by"]
            .as_u64()
            .or_else(|| summary["blocked_by"].as_u64()),
        sub_issues_total: value["sub_issues_summary"]["total"].as_u64(),
    })
}

#[derive(Serialize)]
struct RevisionInput<'a> {
    repository_id: u64,
    node_id: &'a str,
    number: u64,
    title: &'a str,
    state: &'a str,
    body: &'a str,
}
/// Revision of the bounded issue fields; any title, body or state change moves it.
fn revision(repository_id: u64, issue: &Issue) -> String {
    let input = RevisionInput {
        repository_id,
        node_id: &issue.node_id,
        number: issue.number,
        title: &issue.title,
        state: if issue.open { "open" } else { "closed" },
        body: &issue.body,
    };
    let bytes = serde_json::to_vec(&input).unwrap_or_default();
    format!("sha256:{:x}", Sha256::digest(bytes))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskReference {
    pub repository: Option<RepoName>,
    pub number: u64,
    pub pull_request: bool,
}
/// Markdown task-list items (`- [ ]` / `- [x]`) and the issue references in them.
pub fn task_list_references(body: &str, web_host: &str) -> Vec<TaskReference> {
    let mut found = Vec::new();
    for line in body.lines().take(1000) {
        let line = line.trim_start();
        let Some(rest) = ["- [", "* [", "+ ["]
            .iter()
            .find_map(|marker| line.strip_prefix(marker))
        else {
            continue;
        };
        let Some(text) = ["x] ", "X] ", " ] "]
            .iter()
            .find_map(|state| rest.strip_prefix(state))
        else {
            continue;
        };
        for reference in references(text, web_host) {
            if !found.contains(&reference) {
                found.push(reference);
            }
            if found.len() >= MAX_REPORTED_ITEMS {
                return found;
            }
        }
    }
    found
}
fn references(text: &str, web_host: &str) -> Vec<TaskReference> {
    let mut out = Vec::new();
    let prefix = format!("https://{}/", web_host.to_ascii_lowercase());
    for word in text.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | ','))
    {
        let word = word.trim_end_matches(['.', ';', ':']);
        if word.to_ascii_lowercase().starts_with(&prefix) {
            let parts: Vec<_> = word[prefix.len()..].split('/').collect();
            if let [owner, name, kind @ ("issues" | "pull"), number] = parts.as_slice()
                && let (Ok(repository), Ok(number)) = (
                    RepoName::parse(&format!("{owner}/{name}")),
                    number.split('#').next().unwrap_or_default().parse::<u64>(),
                )
                && number > 0
            {
                out.push(TaskReference {
                    repository: Some(repository),
                    number,
                    pull_request: *kind == "pull",
                });
            }
        } else if let Some((left, number)) = word.split_once('#') {
            let Ok(number) = number.parse::<u64>() else {
                continue;
            };
            if number == 0 {
                continue;
            }
            if left.is_empty() {
                out.push(TaskReference {
                    repository: None,
                    number,
                    pull_request: false,
                });
            } else if let Ok(repository) = RepoName::parse(left) {
                out.push(TaskReference {
                    repository: Some(repository),
                    number,
                    pull_request: false,
                });
            }
        }
    }
    out
}
/// List items under an "Acceptance" heading (or bold label) in an issue body.
pub fn acceptance_lines(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut inside = false;
    for line in body.lines().take(1000) {
        let trimmed = line.trim();
        let heading = trimmed.starts_with('#')
            || (trimmed.starts_with("**") && trimmed.ends_with("**") && trimmed.len() > 4);
        if heading {
            inside = trimmed.to_ascii_lowercase().contains("acceptance");
            continue;
        }
        if !inside {
            continue;
        }
        let item = ["- ", "* ", "+ "]
            .iter()
            .find_map(|m| trimmed.strip_prefix(m))
            .or_else(|| {
                let (number, rest) = trimmed.split_once(". ")?;
                (!number.is_empty() && number.bytes().all(|b| b.is_ascii_digit())).then_some(rest)
            });
        let Some(item) = item else {
            continue;
        };
        let item = ["[ ] ", "[x] ", "[X] "]
            .iter()
            .find_map(|m| item.strip_prefix(m))
            .unwrap_or(item);
        if let Some(text) = bounded_text(item, 1000) {
            out.push(text);
        }
        if out.len() >= MAX_ACCEPTANCE_PER_ISSUE {
            break;
        }
    }
    out
}
/// Dependencies documented in prose: references after "blocked by", "blocked on" or
/// "depends on" on the same line, and list items under a "Dependencies" or
/// "Blocked by" heading. These are parsed, not GitHub relations, and are reported
/// as `documented_not_native` unless GitHub also records the relation.
pub fn documented_dependencies(body: &str, web_host: &str) -> Vec<TaskReference> {
    const PHRASES: [&str; 3] = ["blocked by", "blocked on", "depends on"];
    let mut found = Vec::new();
    let mut inside = false;
    for line in body.lines().take(1000) {
        let trimmed = line.trim();
        let lower = trimmed.to_ascii_lowercase();
        let heading = trimmed.starts_with('#')
            || (trimmed.starts_with("**") && trimmed.ends_with("**") && trimmed.len() > 4);
        let mut texts: Vec<&str> = Vec::new();
        if heading {
            inside = lower.contains("dependenc") || lower.contains("blocked by");
        } else if inside
            && let Some(item) = ["- ", "* ", "+ "]
                .iter()
                .find_map(|m| trimmed.strip_prefix(m))
        {
            texts.push(item);
        }
        for phrase in PHRASES {
            // ASCII lowercasing preserves byte offsets into `trimmed`.
            if let Some(at) = lower.find(phrase) {
                texts.push(&trimmed[at + phrase.len()..]);
            }
        }
        for text in texts {
            for reference in references(text, web_host) {
                if !reference.pull_request && !found.contains(&reference) {
                    found.push(reference);
                }
                if found.len() >= MAX_REPORTED_ITEMS {
                    return found;
                }
            }
        }
    }
    found
}
/// Strongly connected components of size > 1, plus self-dependencies.
fn cycles(nodes: &[String], edges: &BTreeMap<String, Vec<String>>) -> Vec<Vec<String>> {
    let index: HashMap<&str, usize> = nodes
        .iter()
        .enumerate()
        .map(|(i, n)| (n.as_str(), i))
        .collect();
    let n = nodes.len();
    let mut reach = vec![vec![false; n]; n];
    for (from, deps) in edges {
        if let Some(&i) = index.get(from.as_str()) {
            for dep in deps {
                if let Some(&j) = index.get(dep.as_str()) {
                    reach[i][j] = true;
                }
            }
        }
    }
    // Transitive closure (Floyd-Warshall); at most 32 nodes.
    for k in 0..n {
        let via = reach[k].clone();
        for row in reach.iter_mut().filter(|row| row[k]) {
            for (cell, reachable) in row.iter_mut().zip(&via) {
                *cell |= *reachable;
            }
        }
    }
    let mut seen = vec![false; n];
    let mut out = Vec::new();
    for i in 0..n {
        if seen[i] || !reach[i][i] {
            continue;
        }
        let mut component: Vec<String> = (0..n)
            .filter(|&j| reach[i][j] && reach[j][i])
            .map(|j| {
                seen[j] = true;
                nodes[j].clone()
            })
            .collect();
        component.sort();
        out.push(component);
    }
    out
}

/// Physical graph binding for intake sources: the graph's opaque local repository
/// identity (what `create_factory_graph` reports as `graph.repository.identity`)
/// plus the approved alias it was computed from. Never a filesystem path.
#[derive(Debug, Clone, Serialize)]
pub struct GraphRepository {
    pub alias: String,
    pub identity: String,
}

/// Forge identity of a GitHub item, kept apart from the physical source binding.
fn forge(repository: &RepoName, issue: &Issue) -> Value {
    json!({"provider":"github","repository":repository.key(),"number":issue.number,"node_id":issue.node_id})
}
/// Canonical forge key, `github:owner/repo#N` (lowercase repository).
pub fn forge_key(repository: &RepoName, number: u64) -> String {
    format!("github:{}#{number}", repository.key())
}
/// Append without duplicates up to the reporting bound; overflow is recorded so
/// the read is never reported as complete.
fn push_bounded(list: &mut Vec<Value>, overflow: &mut bool, entry: Value) {
    if list.contains(&entry) {
        return;
    }
    if list.len() < MAX_REPORTED_ITEMS {
        list.push(entry);
    } else {
        *overflow = true;
    }
}

/// Every way an intake read can be incomplete. Any flag makes the status `incomplete`.
#[derive(Default)]
struct Truncation {
    sub_issues: bool,
    task_list: bool,
    dependencies: bool,
    nodes: bool,
    acceptance: bool,
    bodies: bool,
    reported: bool,
    unrecognized: bool,
}
impl Truncation {
    fn any(&self) -> bool {
        self.sub_issues
            || self.task_list
            || self.dependencies
            || self.nodes
            || self.acceptance
            || self.bodies
            || self.reported
            || self.unrecognized
    }
    fn json(&self) -> Value {
        json!({"sub_issues":self.sub_issues,"task_list":self.task_list,"dependencies":self.dependencies,
            "nodes":self.nodes,"acceptance":self.acceptance,"bodies":self.bodies,
            "reported":self.reported,"unrecognized":self.unrecognized})
    }
}

impl GithubReader {
    pub async fn inspect(
        &self,
        repository: RepoName,
        parent: u64,
        eligibility: Eligibility,
        graph_repository: Option<GraphRepository>,
    ) -> Value {
        let display = self
            .allowed(&repository)
            .unwrap_or_else(|| repository.clone());
        let mut result = json!({
            "schema_version":1,"reported_by":"github","provenance":"github_app","writes":"none",
            "repository":display.full(),"parent":parent,
            "status":"unavailable","reason":null,"retry_at":null,"observed_at":null,
            "eligibility":eligibility,"graph_repository":graph_repository,
            "forge_repository":null,"parent_issue":null,
            "nodes":[],"acceptance_candidates":[],"cycles":[],"external_dependencies":[],
            "cross_repository":[],"omitted":[],
            "truncated":Truncation::default().json(),
            "import":self.import_guidance(false, &["inspection_unavailable"]),
        });
        if let Some(reason) = self.unconfigured_reason() {
            result["reason"] = json!(reason);
            return result;
        }
        let graph = match graph_repository {
            Some(graph) if eligibility.eligible => graph,
            _ => {
                result["status"] = json!("ineligible");
                result["reason"] = json!(
                    eligibility
                        .reasons
                        .first()
                        .copied()
                        .unwrap_or("repository_identity_unavailable")
                );
                result["import"] = self.import_guidance(false, &["repository_ineligible"]);
                return result;
            }
        };
        match self.read_intake(&display, parent, &graph).await {
            Ok(intake) => {
                let object = result.as_object_mut().expect("intake object");
                for (key, value) in intake.as_object().expect("intake fields") {
                    object.insert(key.clone(), value.clone());
                }
            }
            Err(failure) => {
                result["reason"] = json!(failure.reason());
                result["retry_at"] = json!(failure.retry_at());
            }
        }
        result
    }
    fn import_guidance(&self, ready: bool, blockers: &[&str]) -> Value {
        json!({
            "ready":ready,"blockers":blockers,"decisions_needed":[],
            "steps":["create_factory_graph","propose_factory_change","apply_factory_change"],
            "repository_id":"Sources are already bound to graph_repository.identity: create the graph with repository = graph_repository.alias and copy each source unchanged.",
            "confirmation":"explicit_user_confirmation_required"
        })
    }
    async fn read_intake(
        &self,
        repository: &RepoName,
        parent: u64,
        graph: &GraphRepository,
    ) -> Outcome<Value> {
        let base = self.base()?.clone();
        let mut budget = CallBudget::new(INTAKE_REQUEST_BUDGET);
        let scope = Scope::Intake;
        let token = self.token(&mut budget, repository, scope).await?;
        let repository_id = token.repository_id;
        let issues_path = format!("/repos/{}/{}/issues", repository.owner, repository.name);
        let same = |r: &Option<RepoName>| r.as_ref().is_none_or(|r| r.key() == repository.key());

        let reply = self
            .get(
                &mut budget,
                repository,
                scope,
                &token,
                &format!("{issues_path}/{parent}"),
                &[],
            )
            .await?;
        if matches!(reply.status.as_u16(), 301 | 404 | 410) {
            return Err(Failure::Unavailable("parent_issue_unavailable"));
        }
        let parent_issue =
            issue(&reply.ok()?.body).ok_or(Failure::Unavailable("github_invalid_response"))?;
        if parent_issue.pull_request {
            return Err(Failure::Unavailable("parent_is_pull_request"));
        }
        if !same(&parent_issue.repository) {
            return Err(Failure::Unavailable("parent_issue_moved"));
        }

        let mut truncated = Truncation {
            bodies: parent_issue.body_truncated,
            ..Truncation::default()
        };
        let mut candidates: Vec<(Issue, &'static str)> = Vec::new();
        let mut cross = Vec::<Value>::new();
        let mut omitted = Vec::<Value>::new();
        let mut seen = BTreeSet::from([parent]);
        // Native relations come from GitHub's sub-issue and dependency records;
        // task-list and prose references are parsed from issue bodies.
        let cross_entry = |repo: &RepoName, number: u64, relation: &str| {
            let derivation = if matches!(relation, "sub_issue" | "blocked_by") {
                "native"
            } else {
                "parsed"
            };
            json!({"repository":repo.full(),"number":number,"relation":relation,
                "derivation":derivation,"provenance":"github_app","imported":false})
        };
        let omitted_entry = |number: u64, reason: &str| json!({"number":number,"reason":reason});

        // Sub-issues: page-numbered requests only; server-supplied Link URLs are never followed.
        let mut sub_issues_received = 0u64;
        for page in 1..=MAX_SUB_ISSUE_PAGES {
            let reply = self
                .get(
                    &mut budget,
                    repository,
                    scope,
                    &token,
                    &format!("{issues_path}/{parent}/sub_issues"),
                    &[
                        ("per_page", SUB_ISSUES_PER_PAGE.to_string()),
                        ("page", page.to_string()),
                    ],
                )
                .await?;
            if reply.status == StatusCode::NOT_FOUND && page == 1 {
                // No sub-issue record is not the same as no sub-issues.
                truncated.sub_issues = true;
                break;
            }
            let reply = reply.ok()?;
            let items = reply
                .body
                .as_array()
                .ok_or(Failure::Unavailable("github_invalid_response"))?;
            sub_issues_received += items.len() as u64;
            for item in items {
                let Some(child) = issue(item) else {
                    truncated.unrecognized = true;
                    continue;
                };
                if let Some(other) = child
                    .repository
                    .as_ref()
                    .filter(|r| r.key() != repository.key())
                {
                    push_bounded(
                        &mut cross,
                        &mut truncated.reported,
                        cross_entry(other, child.number, "sub_issue"),
                    );
                } else if !seen.insert(child.number) {
                    continue;
                } else if !child.open {
                    push_bounded(
                        &mut omitted,
                        &mut truncated.reported,
                        omitted_entry(child.number, "closed_on_github"),
                    );
                } else if candidates.len() >= MAX_CANDIDATE_NODES {
                    truncated.nodes = true;
                    push_bounded(
                        &mut omitted,
                        &mut truncated.reported,
                        omitted_entry(child.number, "node_limit"),
                    );
                } else {
                    candidates.push((child, "sub_issue"));
                }
            }
            if !reply.has_next {
                break;
            }
            if page == MAX_SUB_ISSUE_PAGES {
                truncated.sub_issues = true;
            }
        }
        // A short page without a next link is still incomplete when GitHub's own
        // summary counts more sub-issues than were returned.
        if parent_issue
            .sub_issues_total
            .is_some_and(|total| total > sub_issues_received)
        {
            truncated.sub_issues = true;
        }

        // Same-repository task-list references in the parent body. Parsed, not native.
        let references = task_list_references(&parent_issue.body, &base.web_host);
        if references.len() >= MAX_REPORTED_ITEMS {
            truncated.task_list = true;
        }
        let mut fetched = 0;
        for reference in references {
            if let Some(other) = reference
                .repository
                .as_ref()
                .filter(|r| r.key() != repository.key())
            {
                push_bounded(
                    &mut cross,
                    &mut truncated.reported,
                    cross_entry(other, reference.number, "task_list"),
                );
                continue;
            }
            if reference.pull_request {
                push_bounded(
                    &mut omitted,
                    &mut truncated.reported,
                    omitted_entry(reference.number, "pull_request_not_imported"),
                );
                continue;
            }
            if !seen.insert(reference.number) {
                continue;
            }
            if fetched >= MAX_TASK_LIST_FETCHES {
                truncated.task_list = true;
                push_bounded(
                    &mut omitted,
                    &mut truncated.reported,
                    omitted_entry(reference.number, "task_list_limit"),
                );
                continue;
            }
            fetched += 1;
            let reply = self
                .get(
                    &mut budget,
                    repository,
                    scope,
                    &token,
                    &format!("{issues_path}/{}", reference.number),
                    &[],
                )
                .await?;
            let child = if matches!(reply.status.as_u16(), 301 | 404 | 410) {
                None
            } else {
                issue(&reply.ok()?.body)
            };
            let reason = match &child {
                None => Some("issue_unavailable"),
                Some(child) if child.pull_request => Some("pull_request_not_imported"),
                Some(child) if !same(&child.repository) => Some("issue_moved"),
                Some(child) if !child.open => Some("closed_on_github"),
                Some(_) if candidates.len() >= MAX_CANDIDATE_NODES => {
                    truncated.nodes = true;
                    Some("node_limit")
                }
                Some(_) => None,
            };
            match (child, reason) {
                (Some(child), None) => candidates.push((child, "task_list")),
                (child, reason) => push_bounded(
                    &mut omitted,
                    &mut truncated.reported,
                    omitted_entry(
                        child.map_or(reference.number, |c| c.number),
                        reason.unwrap_or("issue_unavailable"),
                    ),
                ),
            }
        }

        // Dependencies: native "blocked by" relations where GitHub exposes them, plus
        // dependencies documented in issue prose. An absent native record never erases
        // a documented dependency; it is reported as `documented_not_native` instead.
        let numbers: BTreeSet<u64> = candidates.iter().map(|(i, _)| i.number).collect();
        let mut native: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut documented: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut external = Vec::<Value>::new();
        let mut dependency_api = true;
        for (child, _) in &candidates {
            let id = format!("issue-{}", child.number);
            truncated.bodies |= child.body_truncated;
            let natives = native.entry(id.clone()).or_default();
            let mut native_external = BTreeSet::new();
            if !dependency_api {
                truncated.dependencies = true;
            } else if child.blocked_by_total != Some(0) {
                let reply = self
                    .get(
                        &mut budget,
                        repository,
                        scope,
                        &token,
                        &format!("{issues_path}/{}/dependencies/blocked_by", child.number),
                        &[("per_page", DEPENDENCIES_PER_PAGE.to_string())],
                    )
                    .await?;
                if reply.status == StatusCode::NOT_FOUND {
                    dependency_api = false;
                    truncated.dependencies = true;
                } else {
                    let reply = reply.ok()?;
                    let blockers = reply.body.as_array();
                    if reply.has_next
                        || blockers.is_none()
                        || child
                            .blocked_by_total
                            .is_some_and(|total| total > blockers.map_or(0, |b| b.len() as u64))
                    {
                        truncated.dependencies = true;
                    }
                    for blocker in blockers.into_iter().flatten() {
                        let Some(blocker) = issue(blocker) else {
                            truncated.unrecognized = true;
                            continue;
                        };
                        if let Some(other) = blocker
                            .repository
                            .as_ref()
                            .filter(|r| r.key() != repository.key())
                        {
                            push_bounded(
                                &mut cross,
                                &mut truncated.reported,
                                cross_entry(other, blocker.number, "blocked_by"),
                            );
                        } else if numbers.contains(&blocker.number) {
                            let dep = format!("issue-{}", blocker.number);
                            if !natives.contains(&dep) {
                                natives.push(dep);
                            }
                        } else {
                            native_external.insert(blocker.number);
                            push_bounded(
                                &mut external,
                                &mut truncated.reported,
                                json!({"node_id":id,"blocked_by":blocker.number,
                                    "github_state":if blocker.open {"open"} else {"closed"},
                                    "imported":false,"derivation":"native","provenance":"github_app","status":"native"}),
                            );
                        }
                    }
                }
            }
            let docs = documented.entry(id.clone()).or_default();
            let prose = documented_dependencies(&child.body, &base.web_host);
            if prose.len() >= MAX_REPORTED_ITEMS {
                truncated.dependencies = true;
            }
            for reference in prose {
                match reference.repository.as_ref() {
                    Some(other) if other.key() != repository.key() => push_bounded(
                        &mut cross,
                        &mut truncated.reported,
                        cross_entry(other, reference.number, "documented_dependency"),
                    ),
                    _ if reference.number == child.number => {}
                    _ if numbers.contains(&reference.number) => {
                        let dep = format!("issue-{}", reference.number);
                        if !docs.contains(&dep) {
                            docs.push(dep);
                        }
                    }
                    _ if native_external.contains(&reference.number) => {}
                    _ => push_bounded(
                        &mut external,
                        &mut truncated.reported,
                        json!({"node_id":id,"blocked_by":reference.number,"github_state":null,
                            "imported":false,"derivation":"parsed","provenance":"github_app","status":"documented_not_native"}),
                    ),
                }
            }
        }
        let mut dependencies: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut evidence: BTreeMap<String, Vec<Value>> = BTreeMap::new();
        let mut documented_not_native = false;
        for (child, _) in &candidates {
            let id = format!("issue-{}", child.number);
            let natives = native.get(&id).cloned().unwrap_or_default();
            let docs = documented.get(&id).cloned().unwrap_or_default();
            let mut union = natives.clone();
            union.extend(docs.iter().filter(|d| !natives.contains(d)).cloned());
            let rows = union
                .iter()
                .map(|dep| {
                    let is_native = natives.contains(dep);
                    documented_not_native |= !is_native;
                    json!({"node_id":dep,"native":is_native,"documented":docs.contains(dep),
                        "derivation":if is_native {"native"} else {"parsed"},
                        "status":if is_native {"native"} else {"documented_not_native"}})
                })
                .collect();
            evidence.insert(id.clone(), rows);
            dependencies.insert(id, union);
        }
        let documented_only = documented_not_native
            || external
                .iter()
                .any(|e| e["status"] == "documented_not_native");
        let ids: Vec<String> = candidates
            .iter()
            .map(|(i, _)| format!("issue-{}", i.number))
            .collect();
        let found_cycles = cycles(&ids, &dependencies);

        let source = |issue: &Issue| {
            json!({"provider":"github","repository_id":graph.identity,
                "item_id":format!("{repository_id}:{}", issue.node_id),
                "revision":revision(repository_id, issue),
                "display":{"number":issue.number,"url":base.issue_url(repository, issue.number)}})
        };
        let title = |issue: &Issue| {
            bounded_text(&issue.title, 1000).unwrap_or_else(|| format!("Issue #{}", issue.number))
        };
        let mut acceptance = Vec::<Value>::new();
        let mut add_acceptance = |node: Option<&str>, prefix: &str, body: &str| -> bool {
            let lines = acceptance_lines(body);
            let mut cut = lines.len() >= MAX_ACCEPTANCE_PER_ISSUE;
            for (i, text) in lines.into_iter().enumerate() {
                if acceptance.len() >= MAX_ACCEPTANCE_TOTAL {
                    cut = true;
                    break;
                }
                acceptance.push(
                    json!({"id":format!("{prefix}:acceptance-{}", i + 1),"node_id":node,
                    "text":text,"label":"derived_from_issue","derivation":"parsed","provenance":"github_app"}),
                );
            }
            cut
        };
        let mut acceptance_cut =
            add_acceptance(None, &format!("issue-{parent}"), &parent_issue.body);
        let nodes: Vec<Value> = candidates
            .iter()
            .map(|(issue, relation)| {
                let id = format!("issue-{}", issue.number);
                acceptance_cut |= add_acceptance(Some(&id), &id, &issue.body);
                json!({"id":id,"title":title(issue),
                    "dependencies":dependencies.get(&id).cloned().unwrap_or_default(),
                    "dependency_evidence":evidence.get(&id).cloned().unwrap_or_default(),
                    "relation":relation,"derivation":if *relation == "sub_issue" {"native"} else {"parsed"},
                    "github_state":"open","provenance":"github_app",
                    "forge":forge(repository, issue),"forge_key":forge_key(repository, issue.number),
                    "source":source(issue)})
            })
            .collect();
        truncated.acceptance = acceptance_cut;
        let mut blockers = Vec::new();
        if nodes.is_empty() {
            blockers.push("no_candidate_nodes");
        }
        if !found_cycles.is_empty() {
            blockers.push("dependency_cycle");
        }
        let mut decisions = vec!["confirm_acceptance_criteria", "bind_criteria_to_nodes"];
        if candidates
            .iter()
            .any(|(_, relation)| *relation == "task_list")
        {
            decisions.push("confirm_parsed_task_list_nodes");
        }
        if documented_only {
            decisions.push("confirm_documented_dependencies");
        }
        if !external.is_empty() {
            decisions.push("external_dependencies_not_imported");
        }
        if !cross.is_empty() {
            decisions.push("cross_repository_references_not_imported");
        }
        let complete = !truncated.any();
        if !complete {
            decisions.push("results_incomplete");
        }
        let mut guidance = self.import_guidance(blockers.is_empty(), &blockers);
        guidance["decisions_needed"] = json!(decisions);
        Ok(json!({
            "status":if complete {"complete"} else {"incomplete"},
            "observed_at":crate::store::now(),
            "forge_repository":{"provider":"github","repository":repository.key(),"repository_id":repository_id,"node_id":token.repository_node_id},
            "parent_issue":{"number":parent,"title":title(&parent_issue),"github_state":if parent_issue.open {"open"} else {"closed"},
                "provenance":"github_app","forge":forge(repository, &parent_issue),
                "forge_key":forge_key(repository, parent),"source":source(&parent_issue)},
            "nodes":nodes,"acceptance_candidates":acceptance,
            "cycles":found_cycles.into_iter().map(|ids| json!({"node_ids":ids,"status":"rejected"})).collect::<Vec<_>>(),
            "external_dependencies":external,"cross_repository":cross,"omitted":omitted,"truncated":truncated.json(),
            "import":guidance,
        }))
    }
}

// ----------------------------------------------------------------------------
// #78: PR, check-run, review and diff telemetry. Display only, never proof.

const DELIVERY_QUERY: &str = "query LunaDelivery($id: ID!) { node(id: $id) { __typename ... on Issue { number state repository { databaseId nameWithOwner } closedByPullRequestsReferences(first: 5, includeClosedPrs: true) { totalCount nodes { number state isDraft headRefOid reviewDecision additions deletions changedFiles repository { databaseId nameWithOwner } commits(last: 1) { nodes { commit { oid statusCheckRollup { state contexts(first: 25) { totalCount nodes { __typename ... on CheckRun { name status conclusion startedAt completedAt title summary } ... on StatusContext { context state createdAt } } } } } } } } } } } }";

/// A graph node with a GitHub source binding, as recorded in the Factory ledger.
#[derive(Debug, Clone)]
pub struct DeliverySource {
    pub node_id: String,
    pub item_id: String,
}

fn lower_enum(value: &Value, allowed: &[&str]) -> Value {
    match value.as_str().map(str::to_ascii_lowercase) {
        Some(v) if allowed.contains(&v.as_str()) => json!(v),
        Some(_) => json!("unknown"),
        None => Value::Null,
    }
}
fn timestamp(value: &Value) -> Value {
    json!(value.as_str().and_then(parse_timestamp))
}
fn count_field(value: &Value) -> Value {
    json!(value.as_u64().map(|n| n.min(10_000_000)))
}
fn parse_item_id(item_id: &str) -> Option<(u64, &str)> {
    let (repo, node) = item_id.split_once(':')?;
    let repo = repo
        .bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| repo.parse::<u64>().ok())??;
    (repo > 0 && valid_node_id(node)).then_some((repo, node))
}

impl GithubReader {
    fn pull_request(&self, repository: &RepoName, pr: &Value, observed_at: u64) -> Option<Value> {
        let number = pr["number"].as_u64().filter(|n| *n > 0)?;
        let state = lower_enum(&pr["state"], &["open", "closed", "merged"]);
        let draft = pr["isDraft"].as_bool().unwrap_or(false);
        let head = pr["headRefOid"]
            .as_str()
            .filter(|s| s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()));
        let commit = &pr["commits"]["nodes"][0]["commit"];
        let rollup = &commit["statusCheckRollup"];
        let rollup_state = lower_enum(
            &rollup["state"],
            &["success", "failure", "pending", "error", "expected"],
        );
        let contexts = rollup["contexts"]["nodes"].as_array();
        let total = rollup["contexts"]["totalCount"].as_u64().unwrap_or(0);
        let mut runs = Vec::new();
        let mut statuses = Vec::new();
        for context in contexts.into_iter().flatten().take(MAX_CHECK_CONTEXTS) {
            match context["__typename"].as_str() {
                Some("CheckRun") => {
                    let Some(name) = context["name"].as_str().and_then(|n| bounded_text(n, 200))
                    else {
                        continue;
                    };
                    let detail = context["title"]
                        .as_str()
                        .and_then(|t| bounded_text(t, 280))
                        .or_else(|| {
                            context["summary"]
                                .as_str()
                                .and_then(|s| bounded_text(s, 280))
                        });
                    runs.push(json!({"name":name,
                        "status":lower_enum(&context["status"],&["queued","in_progress","completed","waiting","requested","pending"]),
                        "conclusion":lower_enum(&context["conclusion"],&["success","failure","neutral","cancelled","skipped","timed_out","action_required","stale","startup_failure"]),
                        "started_at":timestamp(&context["startedAt"]),"completed_at":timestamp(&context["completedAt"]),
                        "summary":detail}));
                }
                Some("StatusContext") => {
                    let Some(name) = context["context"]
                        .as_str()
                        .and_then(|n| bounded_text(n, 200))
                    else {
                        continue;
                    };
                    statuses.push(json!({"context":name,
                        "state":lower_enum(&context["state"],&["success","failure","pending","error","expected"]),
                        "created_at":timestamp(&context["createdAt"])}));
                }
                _ => {}
            }
        }
        let ready = state == "open" && !draft && rollup_state == "success";
        Some(json!({
            "number":number,"url":self.base.as_ref()?.pull_url(repository, number),"state":state,"draft":draft,
            "head_sha":head,
            "review_decision":lower_enum(&pr["reviewDecision"],&["approved","changes_requested","review_required"]),
            "diff":{"files_changed":count_field(&pr["changedFiles"]),"additions":count_field(&pr["additions"]),"deletions":count_field(&pr["deletions"])},
            "checks":{"rollup":rollup_state,"total":total.min(10_000),"truncated":total as usize > runs.len() + statuses.len(),
                "runs":runs,"statuses":statuses},
            "ready_for_review":ready,"merge_authority":false,
            "observed_at":observed_at,"reported_by":"github","provenance":"github_app"
        }))
    }
    async fn observe_node(
        &self,
        budget: &mut CallBudget,
        repositories: &[RepoName],
        source: &DeliverySource,
    ) -> Outcome<Value> {
        let (repository_id, node_id) = parse_item_id(&source.item_id)
            .ok_or(Failure::Unavailable("unrecognized_github_item_id"))?;
        let scope = Scope::Delivery;
        let mut selected = None;
        for repository in repositories {
            let token = self.token(budget, repository, scope).await?;
            if token.repository_id == repository_id {
                selected = Some((repository.clone(), token));
                break;
            }
        }
        let (repository, token) =
            selected.ok_or(Failure::Unavailable("source_repository_mismatch"))?;
        let url = self.base()?.endpoint("/graphql")?;
        let reply = self
            .send(
                budget,
                Method::POST,
                url,
                &token.token,
                Some(json!({"query":DELIVERY_QUERY,"variables":{"id":node_id}})),
            )
            .await?;
        if reply.status == StatusCode::UNAUTHORIZED {
            self.evict_token(&repository, scope).await;
        }
        let rate_reset = reply.rate_reset;
        let body = reply.ok()?.body;
        if let Some(errors) = body["errors"].as_array() {
            if errors.iter().any(|e| e["type"] == "RATE_LIMITED") {
                let now = crate::store::now();
                let retry_at = rate_reset
                    .unwrap_or(now + 60)
                    .clamp(now + 1, now + MAX_RATE_LIMIT_WAIT_SECONDS);
                self.state.lock().await.rate_limited_until = Some(retry_at);
                return Err(Failure::RateLimited { retry_at });
            }
            if body["data"]["node"].is_null() {
                return Err(Failure::Unavailable("issue_unavailable"));
            }
        }
        let node = &body["data"]["node"];
        if node.is_null() {
            return Err(Failure::Unavailable("issue_unavailable"));
        }
        if node["__typename"] != "Issue" {
            return Err(Failure::Unavailable("source_not_an_issue"));
        }
        if node["repository"]["databaseId"].as_u64() != Some(repository_id)
            || node["repository"]["nameWithOwner"]
                .as_str()
                .map(str::to_ascii_lowercase)
                != Some(repository.key())
        {
            return Err(Failure::Unavailable("source_repository_mismatch"));
        }
        let number = node["number"]
            .as_u64()
            .filter(|n| *n > 0)
            .ok_or(Failure::Unavailable("github_invalid_response"))?;
        let observed_at = crate::store::now();
        let references = &node["closedByPullRequestsReferences"];
        let mut pulls = Vec::new();
        let mut cross = 0;
        let mut unrecognized = false;
        let listed = references["nodes"].as_array();
        for pr in listed.into_iter().flatten().take(MAX_PULL_REQUESTS) {
            if pr["repository"]["databaseId"].as_u64() != Some(repository_id) {
                cross += 1;
                continue;
            }
            match self.pull_request(&repository, pr, observed_at) {
                Some(pr) => pulls.push(pr),
                None => unrecognized = true,
            }
        }
        let total = references["totalCount"].as_u64();
        let partial = body["errors"].as_array().is_some_and(|e| !e.is_empty());
        let shown = listed.map_or(0, Vec::len) as u64;
        // Complete only when every linked PR and every check context was read.
        let complete = !partial
            && !unrecognized
            && total.is_some_and(|total| total <= MAX_PULL_REQUESTS as u64 && shown >= total)
            && pulls.iter().all(|pr| pr["checks"]["truncated"] == false);
        let total = total.unwrap_or(0);
        let base = self.base()?;
        Ok(json!({
            "partial":partial,"complete":complete,
            "issue":{"number":number,"url":base.issue_url(&repository, number),
                "state":lower_enum(&node["state"],&["open","closed"])},
            "pull_requests":pulls,"pull_requests_total":total.min(10_000),
            "pull_requests_truncated":total > MAX_PULL_REQUESTS as u64,
            "cross_repository_pull_requests":cross,"observed_at":observed_at
        }))
    }

    /// Bounded polling: each node is fetched at most once per minute, at most 16
    /// nodes per call, sequentially, and transport failures stop the call.
    pub async fn delivery(
        &self,
        run_id: &str,
        graph_revision: u64,
        sources: Vec<DeliverySource>,
        repositories: Result<Vec<RepoName>, &'static str>,
    ) -> Value {
        let mut result = json!({
            "schema_version":1,"run_id":run_id,"graph_revision":graph_revision,
            "reported_by":"github","provenance":"github_app","proof":"none","merge_capability":"none",
            "available":false,"complete":false,"reason":null,"retry_at":null,
            "min_interval_seconds":DELIVERY_MIN_INTERVAL_SECONDS,
            "observed_at":crate::store::now(),"nodes":[]
        });
        if let Some(reason) = self.unconfigured_reason() {
            result["reason"] = json!(reason);
            return result;
        }
        if sources.is_empty() {
            result["reason"] = json!("no_github_sources");
            return result;
        }
        let repositories = match repositories {
            Ok(repositories) if !repositories.is_empty() => repositories,
            Ok(_) => {
                result["reason"] = json!("repository_not_in_github_app_allowlist");
                return result;
            }
            Err(reason) => {
                result["reason"] = json!(reason);
                return result;
            }
        };
        let mut budget = CallBudget::new(DELIVERY_REQUEST_BUDGET);
        let mut global: Option<Failure> = None;
        let mut fetches = 0;
        let mut nodes = Vec::new();
        let interval = Duration::from_secs(DELIVERY_MIN_INTERVAL_SECONDS);
        for source in sources.iter().take(128) {
            let key = format!("{run_id}\u{0}{}\u{0}{}", source.node_id, source.item_id);
            let cached = self.state.lock().await.delivery.get(&key).cloned();
            let entry = |reason: Option<&str>, value: Option<&Value>, fresh: Option<&str>| {
                let (status, freshness) = match (value, reason) {
                    (Some(_), None) => ("observed", fresh),
                    (Some(_), Some(_)) => ("observed", Some("stale")),
                    (None, Some("polling_budget_deferred")) => ("deferred", None),
                    (None, _) => ("unavailable", None),
                };
                let mut entry = json!({"node_id":source.node_id,"item_id":source.item_id,"reported_by":"github",
                    "provenance":"github_app","status":status,"freshness":freshness,"reason":reason,"observed_at":null,
                    "issue":null,"pull_requests":[],"pull_requests_total":0,"pull_requests_truncated":false,"complete":false});
                if let Some(data) = value.and_then(Value::as_object) {
                    for (k, v) in data {
                        entry[k] = v.clone();
                    }
                }
                // Stale, deferred or unavailable data is never a complete read.
                if status != "observed" || freshness == Some("stale") {
                    entry["complete"] = json!(false);
                }
                entry
            };
            let previous = cached.as_ref().and_then(|c| c.value.as_ref());
            // Minimum interval per node, for failures as well as observations.
            if let Some(c) = cached.as_ref().filter(|c| c.attempted.elapsed() < interval) {
                nodes.push(entry(c.failure, previous, Some("cached")));
                continue;
            }
            if let Some(failure) = &global {
                nodes.push(entry(Some(failure.reason()), previous, None));
                continue;
            }
            if fetches >= DELIVERY_FETCHES_PER_CALL {
                nodes.push(entry(Some("polling_budget_deferred"), previous, None));
                continue;
            }
            fetches += 1;
            let outcome = self.observe_node(&mut budget, &repositories, source).await;
            let record = match &outcome {
                Ok(value) => CachedDelivery {
                    attempted: Instant::now(),
                    value: Some(value.clone()),
                    failure: None,
                },
                Err(failure) => CachedDelivery {
                    attempted: Instant::now(),
                    value: previous.cloned(),
                    failure: Some(failure.reason()),
                },
            };
            nodes.push(entry(record.failure, record.value.as_ref(), Some("fresh")));
            {
                let mut state = self.state.lock().await;
                if state.delivery.len() >= MAX_DELIVERY_CACHE
                    && !state.delivery.contains_key(&key)
                    && let Some(oldest) = state
                        .delivery
                        .iter()
                        .min_by_key(|(_, c)| c.attempted)
                        .map(|(k, _)| k.clone())
                {
                    state.delivery.remove(&oldest);
                }
                state.delivery.insert(key, record);
            }
            if let Err(failure) = outcome
                && failure.is_global()
            {
                global = Some(failure);
            }
        }
        let complete = global.is_none()
            && sources.len() <= 128
            && nodes.iter().all(|node| node["complete"] == true);
        result["nodes"] = json!(nodes);
        match global {
            Some(failure) => {
                result["reason"] = json!(failure.reason());
                result["retry_at"] = json!(failure.retry_at());
            }
            None => {
                result["available"] = json!(true);
                result["complete"] = json!(complete);
            }
        }
        result
    }
}

// ----------------------------------------------------------------------------
// Workspaces: list the installation's repositories and clone one connected
// repository. Reads only; nothing is pushed, branched or written to GitHub.

const LIST_PER_PAGE: u32 = 100;
const MAX_LIST_PAGES: u32 = 20;
const LIST_REQUEST_BUDGET: u32 = 6;
const CLONE_REQUEST_BUDGET: u32 = 6;
const CLONE_TIMEOUT: Duration = Duration::from_secs(300);
pub const MAX_DESCRIPTION_CHARS: usize = 280;

/// `page:<n>` for pages 2 and later. No cursor means the first page. Cursors are
/// Luna's own; GitHub `Link` URLs are never followed.
pub fn list_cursor_page(cursor: Option<&str>) -> Result<u32> {
    let Some(cursor) = cursor else {
        return Ok(1);
    };
    let page = cursor
        .strip_prefix("page:")
        .filter(|n| !n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|n| n.parse::<u32>().ok())
        .filter(|n| (2..=MAX_LIST_PAGES).contains(n));
    page.ok_or_else(|| anyhow::anyhow!("invalid_github_repository_cursor"))
}
/// The listing envelope; `available` and `repositories` are filled on success.
pub fn repository_list(configured: bool, reason: Option<&str>) -> Value {
    json!({"schema_version":1,"reported_by":"github","provenance":"github_app",
        "configured":configured,"available":false,"reason":reason,"repositories":[],
        "next_cursor":null,"observed_at":crate::store::now()})
}
/// Approved aliases by the lowercase `owner/repo` their Git remotes name.
pub fn remote_aliases(config: &crate::config::Config, web_host: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (alias, repo) in config.repositories.iter().take(128) {
        for remote in remote_repositories(&repo.root, web_host) {
            out.entry(remote.key()).or_insert_with(|| alias.clone());
        }
    }
    out
}
/// The repository `origin` names on `web_host`, if any.
pub fn origin_repository(root: &Path, web_host: &str) -> Option<RepoName> {
    let url = crate::store::git(root, &["config", "--get", "remote.origin.url"]).ok()?;
    parse_remote(&url, web_host)
}
/// The plain HTTPS remote recorded after cloning. It never carries a credential.
pub fn canonical_remote(repository: &RepoName, web_host: &str) -> String {
    format!("https://{web_host}/{}.git", repository.key())
}
/// `git clone` with the token only in the environment, as an `http.extraHeader`
/// read through `GIT_CONFIG_COUNT`. It is never on argv, never written to
/// `.git/config` and never logged. Protocols are pinned and prompts are off.
fn clone_command(
    source: &str,
    branch: Option<&str>,
    token: Option<&Secret>,
    protocol: &str,
) -> std::process::Command {
    use base64::Engine;
    let mut command = crate::workspaces::git_command();
    command.env("GIT_ALLOW_PROTOCOL", protocol);
    let mut entries: Vec<(&str, String)> = vec![
        ("http.followRedirects", "false".into()),
        ("credential.helper", String::new()),
    ];
    if let Some(token) = token {
        let basic =
            base64::engine::general_purpose::STANDARD.encode(format!("x-access-token:{}", token.0));
        entries.push(("http.extraHeader", format!("Authorization: Basic {basic}")));
    }
    command.env("GIT_CONFIG_COUNT", entries.len().to_string());
    for (index, (key, value)) in entries.into_iter().enumerate() {
        command.env(format!("GIT_CONFIG_KEY_{index}"), key);
        command.env(format!("GIT_CONFIG_VALUE_{index}"), value);
    }
    command.args([
        "clone",
        "--quiet",
        "--template=",
        "--no-tags",
        "--single-branch",
        "--depth",
        "1",
        "--no-recurse-submodules",
    ]);
    if let Some(branch) = branch {
        command.arg("--branch").arg(branch);
    }
    // The staging directory is created before token minting and the child enters
    // it with fchdir; clone into `.` so no mutable path is resolved after checks.
    command.arg("--").arg(source).arg(".");
    command
}

impl GithubReader {
    /// Test hook for debug builds against a loopback fake GitHub: clone from
    /// `<dir>/<owner>/<repo>.git` (a local bare repository) instead of GitHub. The
    /// token is still minted and passed exactly as in production. Release builds
    /// and non-loopback API bases refuse it. Not reachable from MCP.
    #[doc(hidden)]
    pub fn use_fixture_clone_source(&self, directory: PathBuf) -> bool {
        cfg!(debug_assertions)
            && self.base.as_ref().is_some_and(|base| base.loopback)
            && self.clone_source.set(directory).is_ok()
    }

    /// Test-only race seam for replacing the workspace pathname while the
    /// already-pinned staging directory remains open.
    #[doc(hidden)]
    pub fn use_clone_hook(&self, hook: Arc<dyn Fn() + Send + Sync>) -> bool {
        cfg!(debug_assertions) && self.clone_hook.set(hook).is_ok()
    }

    /// Test-only seam after Git completes and before lifecycle publication.
    #[doc(hidden)]
    pub fn use_clone_completion_hook(&self, hook: Arc<dyn Fn() + Send + Sync>) -> bool {
        cfg!(debug_assertions) && self.clone_complete_hook.set(hook).is_ok()
    }

    /// One page of the repositories the installation can see. Each entry is marked
    /// connected (an approved alias names it) and connectable (allowlisted, the
    /// installation has `contents: read`, not yet connected).
    pub async fn list_repositories(&self, page: u32, connected: &HashMap<String, String>) -> Value {
        let mut result = repository_list(true, None);
        if let Some(reason) = self.unconfigured_reason() {
            result["configured"] = json!(false);
            result["reason"] = json!(reason);
            return result;
        }
        match self.read_repository_page(page, connected).await {
            Ok((repositories, next)) => {
                result["available"] = json!(true);
                result["repositories"] = json!(repositories);
                result["next_cursor"] = json!(next);
            }
            Err(failure) => result["reason"] = json!(failure.reason()),
        }
        result
    }
    async fn read_repository_page(
        &self,
        page: u32,
        connected: &HashMap<String, String>,
    ) -> Outcome<(Vec<Value>, Option<String>)> {
        let mut budget = CallBudget::new(LIST_REQUEST_BUDGET);
        let token = self.mint(&mut budget, None, Scope::List).await?;
        let mut url = self.base()?.endpoint("/installation/repositories")?;
        url.query_pairs_mut()
            .append_pair("per_page", &LIST_PER_PAGE.to_string())
            .append_pair("page", &page.to_string());
        let reply = self
            .send(&mut budget, Method::GET, url, &token.token, None)
            .await?;
        if reply.status == StatusCode::UNAUTHORIZED {
            self.state
                .lock()
                .await
                .tokens
                .remove(&("*".to_owned(), Scope::List));
        }
        let has_next = reply.has_next;
        let body = reply.ok()?.body;
        let items = body["repositories"]
            .as_array()
            .ok_or(Failure::Unavailable("github_invalid_response"))?;
        let mut repositories = Vec::new();
        for item in items.iter().take(LIST_PER_PAGE as usize) {
            let Some(name) = item["full_name"]
                .as_str()
                .and_then(|full| RepoName::parse(full).ok())
            else {
                continue;
            };
            let key = name.key();
            let connected_alias = connected.get(&key).cloned();
            let blocked_reason = if connected_alias.is_some() {
                None
            } else if self.allowed(&name).is_none() {
                Some("repository_not_in_github_app_allowlist")
            } else if !token.contents_read {
                Some("github_contents_permission_missing")
            } else {
                None
            };
            repositories.push(json!({
                "full_name":key,
                "private":item["private"].as_bool().unwrap_or(true),
                "default_branch":item["default_branch"].as_str().filter(|b| valid_branch(b)),
                "description":item["description"].as_str().and_then(|d| bounded_text(d, MAX_DESCRIPTION_CHARS)),
                "connectable":connected_alias.is_none() && blocked_reason.is_none(),
                "connected_alias":connected_alias,
                "blocked_reason":blocked_reason
            }));
        }
        let next = (has_next && page < MAX_LIST_PAGES).then(|| format!("page:{}", page + 1));
        Ok((repositories, next))
    }

    /// Shallow-clone one allowlisted repository into `target`, which must not
    /// exist. The token is narrowed to that repository with `contents: read` only.
    /// Afterwards `origin` is the plain HTTPS URL and `.git/config` is checked to
    /// hold no credential. Returns the default branch GitHub reported.
    pub async fn clone_repository(
        &self,
        repository: &RepoName,
        staging: &crate::workspaces::PinnedStaging,
        max_bytes: u64,
    ) -> std::result::Result<Option<String>, &'static str> {
        if let Some(reason) = self.unconfigured_reason() {
            return Err(reason);
        }
        let mut budget = CallBudget::new(CLONE_REQUEST_BUDGET);
        let token = self
            .token(&mut budget, repository, Scope::Contents)
            .await
            .map_err(|failure| failure.reason())?;
        if cfg!(debug_assertions)
            && let Some(hook) = self.clone_hook.get()
        {
            hook();
        }
        let web_host = self.web_host().to_owned();
        let (source, protocol) = match self.clone_source.get() {
            Some(directory) => (
                format!(
                    "file://{}/{}/{}.git",
                    directory.display(),
                    repository.owner.to_ascii_lowercase(),
                    repository.name.to_ascii_lowercase()
                ),
                "file",
            ),
            None => (canonical_remote(repository, &web_host), "https"),
        };
        let mut command = clone_command(
            &source,
            token.default_branch.as_deref(),
            Some(&token.token),
            protocol,
        );
        command
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped());
        staging
            .prepare_clone_command(&mut command, max_bytes)
            .map_err(|_| "github_clone_failed")?;
        let mut command = tokio::process::Command::from(command);
        command.kill_on_drop(true);
        let mut child = command.spawn().map_err(|_| "git_unavailable")?;
        let mut process_group = CloneProcessGroup::new(child.id());
        let stderr = child.stderr.take().ok_or("github_clone_failed")?;
        let stderr_task = tokio::spawn(read_clone_stderr(stderr));
        let clone_deadline = tokio::time::Instant::now() + CLONE_TIMEOUT;
        let status =
            wait_for_workspace_clone(&mut child, staging, max_bytes, clone_deadline).await?;
        let stderr = finish_clone_stderr(stderr_task, &process_group, clone_deadline).await?;
        if !status.success() {
            if clone_hit_size_limit(&status, &stderr) {
                return Err("clone_too_large");
            }
            return Err("github_clone_failed");
        }
        if staging.size(max_bytes).is_none_or(|size| size > max_bytes) {
            return Err("clone_too_large");
        }
        let mut set_origin = crate::workspaces::git_command();
        set_origin
            .args(["remote", "set-url", "origin"])
            .arg(canonical_remote(repository, &web_host))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        staging
            .prepare_clone_command(&mut set_origin, max_bytes)
            .map_err(|_| "github_clone_failed")?;
        let status = tokio::process::Command::from(set_origin)
            .status()
            .await
            .map_err(|_| "github_clone_failed")?;
        if !status.success() {
            return Err("github_clone_failed");
        }
        let target = staging.path();
        let config =
            std::fs::read(target.join(".git/config")).map_err(|_| "github_clone_failed")?;
        let text = String::from_utf8_lossy(&config).to_ascii_lowercase();
        if text.contains("extraheader")
            || text.contains("authorization")
            || text.contains(&token.token.0.to_ascii_lowercase())
        {
            return Err("github_clone_credential_persisted");
        }
        if cfg!(debug_assertions)
            && let Some(hook) = self.clone_complete_hook.get()
        {
            hook();
        }
        process_group.disarm();
        Ok(token.default_branch)
    }
}

/// Git may start helpers that outlive the direct child. Keep the clone in its
/// own process group so errors, cancellation, and future drops stop the tree.
struct CloneProcessGroup(Option<u32>);
impl CloneProcessGroup {
    fn new(pid: Option<u32>) -> Self {
        Self(pid)
    }
    fn terminate(&self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            // SAFETY: a negative pid targets only the dedicated clone process group.
            unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
        }
    }
    fn disarm(&mut self) {
        self.0 = None;
    }
}
impl Drop for CloneProcessGroup {
    fn drop(&mut self) {
        self.terminate();
    }
}

async fn wait_for_workspace_clone(
    child: &mut tokio::process::Child,
    staging: &crate::workspaces::PinnedStaging,
    max_bytes: u64,
    deadline: tokio::time::Instant,
) -> std::result::Result<std::process::ExitStatus, &'static str> {
    loop {
        if staging.size(max_bytes).is_none_or(|size| size > max_bytes) {
            kill_clone_group(child.id());
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err("clone_too_large");
        }
        if tokio::time::Instant::now() >= deadline {
            kill_clone_group(child.id());
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err("github_clone_timeout");
        }
        match child.try_wait().map_err(|_| "git_unavailable")? {
            Some(status) if child_was_signaled_by_file_limit(&status) => {
                return Err("clone_too_large");
            }
            Some(status) => return Ok(status),
            None => tokio::time::sleep(Duration::from_millis(100)).await,
        }
    }
}

async fn finish_clone_stderr(
    stderr_task: tokio::task::JoinHandle<Vec<u8>>,
    process_group: &CloneProcessGroup,
    deadline: tokio::time::Instant,
) -> std::result::Result<Vec<u8>, &'static str> {
    let mut stderr_task = stderr_task;
    match tokio::time::timeout_at(deadline, &mut stderr_task).await {
        Ok(result) => Ok(result.unwrap_or_default()),
        Err(_) => {
            process_group.terminate();
            stderr_task.abort();
            Err("github_clone_timeout")
        }
    }
}

fn kill_clone_group(pid: Option<u32>) {
    #[cfg(unix)]
    if let Some(pid) = pid {
        // SAFETY: clone children start a dedicated process group before exec.
        unsafe { libc::kill(-(pid as libc::pid_t), libc::SIGKILL) };
    }
}

async fn read_clone_stderr(mut stderr: tokio::process::ChildStderr) -> Vec<u8> {
    use tokio::io::AsyncReadExt;
    let mut kept = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        match stderr.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(read) => {
                if kept.len() < 16 * 1024 {
                    let take = read.min(16 * 1024 - kept.len());
                    kept.extend_from_slice(&buffer[..take]);
                }
            }
        }
    }
    kept
}

fn clone_hit_file_limit(stderr: &[u8]) -> bool {
    let lower = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    lower.contains("file too large") || lower.contains("file size limit exceeded")
}

fn clone_hit_size_limit(status: &std::process::ExitStatus, stderr: &[u8]) -> bool {
    child_was_signaled_by_file_limit(status) || clone_hit_file_limit(stderr)
}

fn child_was_signaled_by_file_limit(status: &std::process::ExitStatus) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        status.signal() == Some(libc::SIGXFSZ)
    }
    #[cfg(not(unix))]
    {
        let _ = status;
        false
    }
}

pub type SharedReader = Arc<GithubReader>;

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn clone_process_group_drop_kills_descendants_after_leader_exit() {
        use std::{os::unix::process::CommandExt, time::Instant};

        let temp = tempfile::tempdir().unwrap();
        let pid_file = temp.path().join("child.pid");
        let mut command = std::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("sleep 30 & echo $! > \"$1\"; wait")
            .arg("clone-group-test")
            .arg(&pid_file)
            .process_group(0);
        let child = command.spawn().unwrap();
        let group = CloneProcessGroup::new(Some(child.id()));
        let deadline = Instant::now() + Duration::from_secs(2);
        while !pid_file.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        let pid: u32 = std::fs::read_to_string(&pid_file)
            .expect("child pid was written")
            .trim()
            .parse()
            .unwrap();
        drop(group);
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let state = std::fs::read_to_string(format!("/proc/{pid}/stat"))
                .ok()
                .and_then(|stat| {
                    stat.rsplit_once(')')
                        .map(|(_, tail)| tail.trim().chars().next())
                })
                .flatten();
            if state.is_none() || state == Some('Z') {
                break;
            }
            assert!(Instant::now() < deadline, "clone descendant is still alive");
            std::thread::sleep(Duration::from_millis(5));
        }
        // Reap the group leader after the assertion; Drop already killed the group.
        let mut child = child;
        let _ = child.wait();
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn stderr_drain_deadline_kills_descendant_after_clone_leader_exit() {
        use std::{
            os::unix::process::CommandExt,
            time::{Duration, Instant},
        };
        use tokio::io::AsyncReadExt;

        let mut command = std::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("sleep 30 >/dev/null & echo $!; exit 0")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .process_group(0);
        let mut child = tokio::process::Command::from(command).spawn().unwrap();
        let process_group = CloneProcessGroup::new(child.id());
        let stderr = child.stderr.take().unwrap();
        let stderr_task = tokio::spawn(read_clone_stderr(stderr));
        let status = child.wait().await.unwrap();
        assert!(status.success());
        let mut stdout = String::new();
        child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .await
            .unwrap();
        let descendant: u32 = stdout.trim().parse().unwrap();

        let started = Instant::now();
        let result = finish_clone_stderr(
            stderr_task,
            &process_group,
            tokio::time::Instant::now() + Duration::from_millis(100),
        )
        .await;
        assert_eq!(result.unwrap_err(), "github_clone_timeout");
        assert!(started.elapsed() < Duration::from_secs(1));
        let stop_deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let state = std::fs::read_to_string(format!("/proc/{descendant}/stat"))
                .ok()
                .and_then(|stat| {
                    stat.rsplit_once(')')
                        .map(|(_, tail)| tail.trim().chars().next())
                })
                .flatten();
            if state.is_none() || state == Some('Z') {
                break;
            }
            assert!(
                Instant::now() < stop_deadline,
                "descendant remains alive in state {state:?}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[cfg(unix)]
    #[test]
    fn generic_unpack_failure_does_not_imply_a_size_limit() {
        use std::os::unix::process::ExitStatusExt;

        let unrelated_failure = std::process::ExitStatus::from_raw(128 << 8);
        assert!(!clone_hit_size_limit(
            &unrelated_failure,
            b"fatal: fetch-pack: invalid index-pack output"
        ));
        assert!(clone_hit_size_limit(
            &unrelated_failure,
            b"error: File too large"
        ));
        let terminated = std::process::ExitStatus::from_raw(libc::SIGTERM);
        assert!(!clone_hit_size_limit(&terminated, b""));
    }

    #[test]
    fn repository_names_and_api_bases_are_bounded() {
        for good in ["joshyorko/plugins", "a-b/c.d_e", "A/B"] {
            RepoName::parse(good).unwrap();
        }
        for bad in [
            "",
            "owner",
            "owner/",
            "/repo",
            "-owner/repo",
            "owner/repo/extra",
            "owner/..",
            "own er/repo",
            "owner/re#po",
        ] {
            assert!(RepoName::parse(bad).is_err(), "{bad}");
        }
        for good in [
            "https://api.github.com",
            "https://api.example.ghe.com",
            "http://127.0.0.1:8080",
            "http://[::1]:9",
        ] {
            ApiBase::parse(good).unwrap();
        }
        for bad in [
            "http://api.github.com",
            "https://api.github.com/",
            "https://api.github.com/api/v3",
            "https://user:pw@api.github.com",
            "https://api.github.com?x=1",
            "http://localhost:8080",
            "http://127.0.0.1",
            "ftp://api.github.com",
        ] {
            assert!(ApiBase::parse(bad).is_err(), "{bad}");
        }
        assert_eq!(
            ApiBase::parse("https://api.github.com").unwrap().web_host,
            "github.com"
        );
        assert_eq!(
            ApiBase::parse("https://api.acme.ghe.com").unwrap().web_host,
            "acme.ghe.com"
        );
        // Release builds refuse plain HTTP outright, loopback included.
        for http in ["http://127.0.0.1:8080", "http://[::1]:9"] {
            assert!(ApiBase::parse_with(http, false).is_err(), "{http}");
        }
        ApiBase::parse_with("https://api.github.com", false).unwrap();
    }

    #[test]
    fn remotes_task_lists_and_acceptance_are_parsed_conservatively() {
        for url in [
            "https://github.com/joshyorko/plugins.git",
            "https://github.com/joshyorko/plugins",
            "https://token@github.com/joshyorko/plugins/",
            "ssh://git@github.com/joshyorko/plugins.git",
            "git@github.com:joshyorko/plugins.git",
            "git@GitHub.com:JoshYorko/Plugins.git",
        ] {
            assert_eq!(
                parse_remote(url, "github.com").map(|r| r.key()),
                Some("joshyorko/plugins".into()),
                "{url}"
            );
        }
        for url in [
            "https://gitlab.com/joshyorko/plugins.git",
            "https://github.com.evil.example/joshyorko/plugins",
            "/srv/git/plugins.git",
            "git@github.com:joshyorko/plugins/extra.git",
        ] {
            assert!(parse_remote(url, "github.com").is_none(), "{url}");
        }
        let body = "Intro #9 is not a task\n- [ ] #12 first\n- [x] other/repo#3\n* [ ] https://github.com/joshyorko/plugins/issues/14.\n- [ ] https://github.com/joshyorko/plugins/pull/15\n- plain #16\n- [ ] #12 duplicate";
        let refs = task_list_references(body, "github.com");
        assert_eq!(
            refs.iter()
                .map(|r| (
                    r.repository.as_ref().map(RepoName::key),
                    r.number,
                    r.pull_request
                ))
                .collect::<Vec<_>>(),
            vec![
                (None, 12, false),
                (Some("other/repo".into()), 3, false),
                (Some("joshyorko/plugins".into()), 14, false),
                (Some("joshyorko/plugins".into()), 15, true),
            ]
        );
        let body = "## Scope\n- not acceptance\n## Acceptance\n- [ ] Tests pass\n- Docs updated\n1. Third item\n- ghp_secretvalue leaked\n## Notes\n- after";
        assert_eq!(
            acceptance_lines(body),
            vec!["Tests pass", "Docs updated", "Third item"]
        );
        let body = "Depends on #4 and #5.\nPart of #67\nBlocked on https://github.com/joshyorko/plugins/issues/6\nblocked by https://github.com/joshyorko/plugins/pull/7\n## Dependencies\n- other/repo#8\n- #4 again\n## Notes\n- #9 not a dependency";
        assert_eq!(
            documented_dependencies(body, "github.com")
                .iter()
                .map(|r| (r.repository.as_ref().map(RepoName::key), r.number))
                .collect::<Vec<_>>(),
            vec![
                (None, 4),
                (None, 5),
                (Some("joshyorko/plugins".into()), 6),
                (Some("other/repo".into()), 8),
            ]
        );
        assert_eq!(
            forge_key(&RepoName::parse("JoshYorko/Plugins").unwrap(), 67),
            "github:joshyorko/plugins#67"
        );
    }

    #[test]
    fn cycles_and_timestamps_are_exact() {
        let nodes: Vec<String> = ["a", "b", "c", "d"].iter().map(|s| s.to_string()).collect();
        let edges = BTreeMap::from([
            ("a".to_string(), vec!["b".to_string()]),
            ("b".to_string(), vec!["a".to_string()]),
            ("c".to_string(), vec!["c".to_string()]),
            ("d".to_string(), vec!["a".to_string()]),
        ]);
        assert_eq!(
            cycles(&nodes, &edges),
            vec![
                vec!["a".to_string(), "b".to_string()],
                vec!["c".to_string()]
            ]
        );
        assert_eq!(parse_timestamp("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_timestamp("2026-10-10T20:22:10Z"), Some(1_791_663_730));
        assert_eq!(parse_timestamp("2026-10-10T20:22:10+00:00"), None);
        assert_eq!(parse_timestamp("2026-13-10T20:22:10Z"), None);
    }

    #[test]
    fn rate_limits_map_to_a_bounded_retry_time() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-remaining", "0".parse().unwrap());
        headers.insert("x-ratelimit-reset", "1000".parse().unwrap());
        assert_eq!(rate_limit(StatusCode::FORBIDDEN, &headers, 900), Some(1000));
        assert_eq!(
            rate_limit(StatusCode::FORBIDDEN, &HeaderMap::new(), 900),
            None
        );
        let mut secondary = HeaderMap::new();
        secondary.insert("retry-after", "30".parse().unwrap());
        assert_eq!(
            rate_limit(StatusCode::TOO_MANY_REQUESTS, &secondary, 900),
            Some(930)
        );
        headers.insert("x-ratelimit-reset", "99999999".parse().unwrap());
        assert_eq!(
            rate_limit(StatusCode::FORBIDDEN, &headers, 900),
            Some(900 + MAX_RATE_LIMIT_WAIT_SECONDS)
        );
    }

    #[test]
    fn owner_wildcards_admit_only_that_owner_and_reject_bad_entries() {
        let config: GithubAppConfig = serde_json::from_value(json!({
            "app_id":1,"private_key_path":"/k.pem",
            "allowed_repositories":["JoshYorko/*","other/Exact"]
        }))
        .unwrap();
        config.validate().unwrap();
        let allowed = |name: &str| config.allowed(&RepoName::parse(name).unwrap());
        assert_eq!(
            allowed("joshyorko/anything").unwrap().full(),
            "joshyorko/anything"
        );
        assert_eq!(allowed("other/exact").unwrap().full(), "other/Exact");
        assert!(allowed("other/else").is_none());
        assert!(allowed("joshyorkox/plugins").is_none());
        for bad in [
            json!(["*/*"]),
            json!(["owner/*", "OWNER/*"]),
            json!(["-owner/*"]),
            json!(["owner/**"]),
            json!(["owner/"]),
        ] {
            let parsed: GithubAppConfig = serde_json::from_value(
                json!({"app_id":1,"private_key_path":"/k.pem","allowed_repositories":bad.clone()}),
            )
            .unwrap();
            assert!(parsed.validate().is_err(), "{bad}");
        }
    }

    #[test]
    fn clone_tokens_stay_off_argv_and_cursors_and_branches_are_bounded() {
        let token = Secret("ghs_clonesecret".into());
        let command = clone_command(
            "https://github.com/joshyorko/plugins.git",
            Some("main"),
            Some(&token),
            "https",
        );
        let args: Vec<String> = command
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        use base64::Engine;
        let encoded =
            base64::engine::general_purpose::STANDARD.encode("x-access-token:ghs_clonesecret");
        for arg in &args {
            assert!(
                !arg.contains("ghs_clonesecret") && !arg.contains(&encoded),
                "{arg}"
            );
        }
        assert!(args.contains(&"--depth".to_owned()) && args.contains(&"main".to_owned()));
        let envs: HashMap<String, String> = command
            .get_envs()
            .filter_map(|(k, v)| {
                Some((
                    k.to_string_lossy().into_owned(),
                    v?.to_string_lossy().into_owned(),
                ))
            })
            .collect();
        assert_eq!(envs["GIT_ALLOW_PROTOCOL"], "https");
        assert_eq!(envs["GIT_TERMINAL_PROMPT"], "0");
        assert_eq!(envs["GIT_CONFIG_GLOBAL"], "/dev/null");
        let count: usize = envs["GIT_CONFIG_COUNT"].parse().unwrap();
        let header = (0..count)
            .find(|i| envs[&format!("GIT_CONFIG_KEY_{i}")] == "http.extraHeader")
            .unwrap();
        assert_eq!(
            envs[&format!("GIT_CONFIG_VALUE_{header}")],
            format!("Authorization: Basic {encoded}")
        );
        assert_eq!(list_cursor_page(None).unwrap(), 1);
        assert_eq!(list_cursor_page(Some("page:2")).unwrap(), 2);
        for bad in [
            "page:1",
            "page:0",
            "page:21",
            "page:x",
            "2",
            "page:",
            "page:0002",
        ] {
            assert!(list_cursor_page(Some(bad)).is_err(), "{bad}");
        }
        for good in ["main", "release/1.x", "v2_dev"] {
            assert!(valid_branch(good), "{good}");
        }
        for bad in [
            "", "-x", "a..b", "x.lock", "/a", "a/", "a b", "a@{1}", "a//b",
        ] {
            assert!(!valid_branch(bad), "{bad}");
        }
    }

    #[test]
    fn configuration_debug_never_prints_the_key_path() {
        let config: GithubAppConfig = serde_json::from_value(json!({
            "app_id":5266798,"private_key_path":"/run/luna-config/private-key-fixture.pem",
            "allowed_repositories":["joshyorko/plugins"]
        }))
        .unwrap();
        config.validate().unwrap();
        assert_eq!(config.api_base, DEFAULT_API_BASE);
        assert!(!format!("{config:?}").contains("private-key-fixture"));
        assert!(!format!("{:?}", Secret("ghs_value".into())).contains("ghs_value"));
        for bad in [
            json!({"app_id":0,"private_key_path":"/k.pem","allowed_repositories":["a/b"]}),
            json!({"app_id":1,"private_key_path":"k.pem","allowed_repositories":["a/b"]}),
            json!({"app_id":1,"private_key_path":"/x/../k.pem","allowed_repositories":["a/b"]}),
            json!({"app_id":1,"private_key_path":"/k.pem","allowed_repositories":[]}),
            json!({"app_id":1,"private_key_path":"/k.pem","allowed_repositories":["a/b","A/B"]}),
            json!({"app_id":1,"private_key_path":"/k.pem","allowed_repositories":["a/b"],"api_base":"http://api.github.com"}),
            json!({"app_id":1,"installation_id":0,"private_key_path":"/k.pem","allowed_repositories":["a/b"]}),
        ] {
            let parsed: GithubAppConfig = serde_json::from_value(bad.clone()).unwrap();
            assert!(parsed.validate().is_err(), "{bad}");
        }
        assert!(
            serde_json::from_value::<GithubAppConfig>(json!({"app_id":1,"private_key_path":"/k.pem","allowed_repositories":["a/b"],"token":"x"}))
                .is_err()
        );
    }
}
