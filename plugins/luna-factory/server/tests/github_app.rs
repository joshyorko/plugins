//! Recorded-fixture tests for the opt-in GitHub App reader (#76 intake, #78 delivery).
//!
//! A fake GitHub API runs on loopback. No live GitHub request is made. RSA keys are
//! throwaway keys generated per test process with the `openssl` CLI and never
//! committed. Fixture shapes in `tests/fixtures/github` were recorded read-only
//! with `gh api` from joshyorko/plugins issue #67's sub-issues and PR #68.
use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use luna_factoryd::{config::Config, http::McpServer, lifecycle::Factory, mcp::tool_definitions};
use serde_json::{Value, json};
use std::{
    collections::{BTreeSet, HashMap},
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Mutex, OnceLock},
};

const REPO: &str = "joshyorko/plugins";
const REPO_ID: u64 = 1_148_934_299;
const REPO_NODE: &str = "R_kgDORHtYmw";
const APP_ID: u64 = 5_266_798;
const INSTALLATION: u64 = 170_065_829;
const TOKEN_PREFIX: &str = "ghs_fixturetoken";

struct Keys {
    pem: Vec<u8>,
    public_der: Vec<u8>,
}
/// A throwaway 2048-bit key, generated once per test process.
fn keys() -> &'static Keys {
    static KEYS: OnceLock<Keys> = OnceLock::new();
    KEYS.get_or_init(|| {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("throwaway.pem");
        let traditional = Command::new("openssl")
            .args(["genrsa", "-traditional", "-out"])
            .arg(&key)
            .arg("2048")
            .output()
            .is_ok_and(|o| o.status.success());
        if !traditional {
            assert!(
                Command::new("openssl")
                    .args(["genrsa", "-out"])
                    .arg(&key)
                    .arg("2048")
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
        let public = dir.path().join("public.der");
        assert!(
            Command::new("openssl")
                .args(["rsa", "-in"])
                .arg(&key)
                .args(["-RSAPublicKey_out", "-outform", "DER", "-out"])
                .arg(&public)
                .output()
                .unwrap()
                .status
                .success()
        );
        let pem = std::fs::read(&key).unwrap();
        assert!(String::from_utf8_lossy(&pem).contains("BEGIN RSA PRIVATE KEY"));
        Keys {
            pem,
            public_der: std::fs::read(&public).unwrap(),
        }
    })
}

#[derive(Clone, Debug)]
struct Hit {
    method: String,
    path: String,
    query: String,
    credential: String,
    body: Value,
}
struct World {
    issues: HashMap<u64, Value>,
    gone: BTreeSet<u64>,
    sub_issues: HashMap<u64, Vec<Value>>,
    blocked_by: HashMap<u64, Vec<Value>>,
    /// The issue dependencies API is absent (404), as on instances without it.
    dependencies_unavailable: bool,
    graphql: HashMap<String, Value>,
    installation_id: u64,
    installation_permissions: Value,
    token_ttl: i64,
    token_permissions: Option<Value>,
    /// Repositories the minted token claims to cover (default: only REPO).
    token_repositories: Option<Value>,
    /// Issue reads fail with a 500 whose body echoes the caller's credential.
    echo_credential_errors: bool,
    rate_limited: Option<u64>,
    graphql_rate_limited: Option<u64>,
    mints: u32,
    hits: Vec<Hit>,
    jwt_claims: Vec<Value>,
    jwt_headers: Vec<Value>,
    /// Repositories the installation can see (`GET /installation/repositories`),
    /// in addition to REPO for installation lookups.
    repositories: Vec<Value>,
    /// Extra installations reported by `GET /app/installations`.
    extra_installations: u32,
}
impl Default for World {
    fn default() -> Self {
        Self {
            issues: HashMap::new(),
            gone: BTreeSet::new(),
            sub_issues: HashMap::new(),
            blocked_by: HashMap::new(),
            dependencies_unavailable: false,
            graphql: HashMap::new(),
            installation_id: INSTALLATION,
            installation_permissions: json!({"actions":"read","checks":"read","issues":"read","metadata":"read","pull_requests":"read","statuses":"read"}),
            token_ttl: 3600,
            token_permissions: None,
            token_repositories: None,
            echo_credential_errors: false,
            rate_limited: None,
            graphql_rate_limited: None,
            mints: 0,
            hits: vec![],
            jwt_claims: vec![],
            jwt_headers: vec![],
            repositories: vec![],
            extra_installations: 0,
        }
    }
}
/// A repository object with the fields GitHub returns in listings and token grants.
fn repository(full_name: &str, id: u64, description: Option<&str>) -> Value {
    let (_, name) = full_name.split_once('/').unwrap();
    json!({"id":id,"node_id":format!("R_fixture{id}"),"name":name,"full_name":full_name,
        "private":id.is_multiple_of(2),"default_branch":"main","description":description,"archived":false})
}
fn visible(world: &World, full_name: &str) -> Option<Value> {
    world
        .repositories
        .iter()
        .find(|r| {
            r["full_name"]
                .as_str()
                .unwrap()
                .eq_ignore_ascii_case(full_name)
        })
        .cloned()
}
type Shared = Arc<Mutex<World>>;

fn now() -> u64 {
    luna_factoryd::store::now()
}
fn iso(seconds: u64) -> String {
    // Civil-from-days (inverse of the reader's parser), UTC only.
    let days = (seconds / 86_400) as i64;
    let rem = seconds % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60
    )
}
fn reply(status: StatusCode, headers: &[(&str, String)], body: Value) -> Response {
    let mut response = (status, body.to_string()).into_response();
    response
        .headers_mut()
        .insert("content-type", "application/json".parse().unwrap());
    for (name, value) in headers {
        response.headers_mut().insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
    }
    response
}
fn verify_jwt(world: &mut World, token: &str) -> bool {
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.set_required_spec_claims(&["exp", "iat", "iss"]);
    validation.leeway = 0;
    let key = jsonwebtoken::DecodingKey::from_rsa_der(&keys().public_der);
    match jsonwebtoken::decode::<Value>(token, &key, &validation) {
        Ok(data) => {
            world.jwt_claims.push(data.claims);
            world
                .jwt_headers
                .push(serde_json::to_value(&data.header).unwrap());
            true
        }
        Err(_) => false,
    }
}
fn query_value(query: &str, name: &str) -> Option<usize> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(key, _)| *key == name)
        .and_then(|(_, value)| value.parse().ok())
}
async fn handle(
    State(world): State<Shared>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let credential = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    let path = uri.path().to_owned();
    let query = uri.query().unwrap_or_default().to_owned();
    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    let mut world = world.lock().unwrap();
    world.hits.push(Hit {
        method: method.to_string(),
        path: path.clone(),
        query: query.clone(),
        credential: credential.clone(),
        body: body.clone(),
    });
    if let Some(reset) = world.rate_limited {
        return reply(
            StatusCode::FORBIDDEN,
            &[
                ("x-ratelimit-remaining", "0".into()),
                ("x-ratelimit-reset", reset.to_string()),
            ],
            json!({"message":"API rate limit exceeded"}),
        );
    }
    let bearer = credential.strip_prefix("Bearer ").unwrap_or_default();
    let token_ok = bearer.starts_with(TOKEN_PREFIX);
    let unauthorized = || {
        reply(
            StatusCode::UNAUTHORIZED,
            &[],
            json!({"message":"Bad credentials"}),
        )
    };
    let segments: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match (method.as_str(), segments.as_slice()) {
        ("GET", ["repos", owner, name, "installation"]) => {
            if !verify_jwt(&mut world, bearer) {
                return unauthorized();
            }
            let full = format!("{owner}/{name}");
            if full.to_ascii_lowercase() != REPO && visible(&world, &full).is_none() {
                return reply(StatusCode::NOT_FOUND, &[], json!({"message":"Not Found"}));
            }
            reply(
                StatusCode::OK,
                &[],
                json!({"id":world.installation_id,"app_id":APP_ID,"repository_selection":"all",
                    "permissions":world.installation_permissions,"account":{"login":"joshyorko"}}),
            )
        }
        ("POST", ["app", "installations", id, "access_tokens"]) => {
            if !verify_jwt(&mut world, bearer) {
                return unauthorized();
            }
            if id.parse::<u64>().ok() != Some(world.installation_id) {
                return reply(StatusCode::NOT_FOUND, &[], json!({"message":"Not Found"}));
            }
            world.mints += 1;
            let permissions = world
                .token_permissions
                .clone()
                .unwrap_or_else(|| body["permissions"].clone());
            let expires = (now() as i64 + world.token_ttl).max(0) as u64;
            let requested = body["repositories"][0].as_str().map(str::to_owned);
            let narrowed = requested.as_ref().and_then(|name| {
                world
                    .repositories
                    .iter()
                    .find(|r| r["name"].as_str().unwrap().eq_ignore_ascii_case(name))
                    .cloned()
            });
            let repositories = world.token_repositories.clone().unwrap_or_else(|| match narrowed {
                Some(found) => json!([found]),
                None => json!([{"id":REPO_ID,"node_id":REPO_NODE,"name":"plugins","full_name":REPO,"private":false}]),
            });
            let mut granted = json!({"token":format!("{TOKEN_PREFIX}{:04}", world.mints),"expires_at":iso(expires),
                "permissions":permissions,"repository_selection":"selected"});
            // An unnarrowed (listing) token reports no repository list.
            if requested.is_some() || world.token_repositories.is_some() {
                granted["repositories"] = repositories;
            }
            reply(StatusCode::CREATED, &[], granted)
        }
        ("GET", ["app", "installations", id]) => {
            if !verify_jwt(&mut world, bearer) {
                return unauthorized();
            }
            if id.parse::<u64>().ok() != Some(world.installation_id) {
                return reply(StatusCode::NOT_FOUND, &[], json!({"message":"Not Found"}));
            }
            reply(
                StatusCode::OK,
                &[],
                json!({"id":world.installation_id,"app_id":APP_ID,"repository_selection":"all",
                    "permissions":world.installation_permissions,"account":{"login":"joshyorko"}}),
            )
        }
        ("GET", ["app", "installations"]) => {
            if !verify_jwt(&mut world, bearer) {
                return unauthorized();
            }
            let mut all = vec![json!({"id":world.installation_id,"app_id":APP_ID,
                "repository_selection":"all","permissions":world.installation_permissions})];
            for extra in 0..world.extra_installations {
                all.push(
                    json!({"id":900 + extra,"app_id":APP_ID,"permissions":{"metadata":"read"}}),
                );
            }
            reply(StatusCode::OK, &[], json!(all))
        }
        ("GET", ["installation", "repositories"]) => {
            if !token_ok {
                return unauthorized();
            }
            let per_page = query_value(&query, "per_page").unwrap_or(30);
            let page = query_value(&query, "page").unwrap_or(1).max(1);
            let all = &world.repositories;
            let start = ((page - 1) * per_page).min(all.len());
            let end = (start + per_page).min(all.len());
            let mut headers = vec![];
            if end < all.len() {
                headers.push((
                    "link",
                    format!("<https://api.github.com/installation/repositories?per_page={per_page}&page={}>; rel=\"next\"", page + 1),
                ));
            }
            reply(
                StatusCode::OK,
                &headers,
                json!({"total_count":all.len(),"repositories":all[start..end]}),
            )
        }
        ("GET", ["repos", _, _, "issues", number, rest @ ..]) => {
            if !token_ok {
                return unauthorized();
            }
            if world.echo_credential_errors {
                return reply(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &[],
                    json!({"message":format!("upstream failure for {credential}"),"documentation_url":credential}),
                );
            }
            let Ok(number) = number.parse::<u64>() else {
                return reply(StatusCode::NOT_FOUND, &[], json!({}));
            };
            match rest {
                [] => {
                    if world.gone.contains(&number) {
                        return reply(
                            StatusCode::GONE,
                            &[],
                            json!({"message":"This issue was deleted"}),
                        );
                    }
                    match world.issues.get(&number) {
                        Some(issue) => reply(StatusCode::OK, &[], issue.clone()),
                        None => reply(StatusCode::NOT_FOUND, &[], json!({"message":"Not Found"})),
                    }
                }
                ["sub_issues"] => {
                    let all = world.sub_issues.get(&number).cloned().unwrap_or_default();
                    let per_page = query_value(&query, "per_page").unwrap_or(30);
                    let page = query_value(&query, "page").unwrap_or(1).max(1);
                    let start = ((page - 1) * per_page).min(all.len());
                    let end = (start + per_page).min(all.len());
                    let mut headers = vec![];
                    if end < all.len() {
                        headers.push((
                            "link",
                            format!("<https://api.github.com/repositories/{REPO_ID}/issues/{number}/sub_issues?per_page={per_page}&page={}>; rel=\"next\"", page + 1),
                        ));
                    }
                    reply(StatusCode::OK, &headers, json!(all[start..end]))
                }
                ["dependencies", "blocked_by"] if world.dependencies_unavailable => {
                    reply(StatusCode::NOT_FOUND, &[], json!({"message":"Not Found"}))
                }
                ["dependencies", "blocked_by"] => reply(
                    StatusCode::OK,
                    &[],
                    json!(world.blocked_by.get(&number).cloned().unwrap_or_default()),
                ),
                _ => reply(StatusCode::NOT_FOUND, &[], json!({})),
            }
        }
        ("POST", ["graphql"]) => {
            if !token_ok {
                return unauthorized();
            }
            if let Some(reset) = world.graphql_rate_limited {
                return reply(
                    StatusCode::OK,
                    &[("x-ratelimit-reset", reset.to_string())],
                    json!({"errors":[{"type":"RATE_LIMITED","message":"API rate limit exceeded"}]}),
                );
            }
            let id = body["variables"]["id"].as_str().unwrap_or_default();
            match world.graphql.get(id) {
                Some(node) => reply(StatusCode::OK, &[], json!({"data":{"node":node}})),
                None => reply(
                    StatusCode::OK,
                    &[],
                    json!({"data":{"node":null},"errors":[{"type":"NOT_FOUND","message":"Could not resolve"}]}),
                ),
            }
        }
        _ => reply(StatusCode::NOT_FOUND, &[], json!({"message":"Not Found"})),
    }
}
async fn serve(world: Shared) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new().fallback(handle).with_state(world);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    format!("http://{address}")
}

/// Same field set as the recorded REST issue shape.
fn issue(
    repository: &str,
    number: u64,
    title: &str,
    body: &str,
    open: bool,
    blocked_by: u64,
) -> Value {
    json!({
        "url":format!("https://api.github.com/repos/{repository}/issues/{number}"),
        "repository_url":format!("https://api.github.com/repos/{repository}"),
        "html_url":format!("https://github.com/{repository}/issues/{number}"),
        "id":5_794_000_000_u64 + number,"node_id":format!("I_kwDOfixture{number:05}"),"number":number,
        "title":title,"user":{"login":"joshyorko","id":54_248_591,"type":"User"},"labels":[],
        "state":if open {"open"} else {"closed"},"state_reason":null,"locked":false,"assignee":null,
        "assignees":[],"milestone":null,"comments":0,"created_at":"2026-10-10T16:00:00Z",
        "updated_at":"2026-10-10T16:00:00Z","closed_at":null,"author_association":"OWNER",
        "active_lock_reason":null,"sub_issues_summary":{"total":0,"completed":0,"percent_completed":0},
        "issue_dependencies_summary":{"blocked_by":blocked_by,"total_blocked_by":blocked_by,"blocking":0,"total_blocking":0},
        "body":body,"performed_via_github_app":null,"parent_issue_url":null
    })
}
fn recorded(name: &str) -> Value {
    serde_json::from_str(
        &std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/github")
                .join(name),
        )
        .unwrap(),
    )
    .unwrap()
}

struct Harness {
    _temp: tempfile::TempDir,
    server: McpServer,
    world: Shared,
    key_path: PathBuf,
}
impl Harness {
    fn hits(&self) -> Vec<Hit> {
        self.world.lock().unwrap().hits.clone()
    }
    fn mints(&self) -> Vec<Hit> {
        self.hits()
            .into_iter()
            .filter(|h| h.path.ends_with("/access_tokens"))
            .collect()
    }
}
fn git(root: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .unwrap()
            .status
            .success(),
        "{args:?}"
    );
}
async fn harness(world: World, configure: impl FnOnce(&mut Value, &str)) -> Harness {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--allow-empty",
            "-qm",
            "fixture",
        ],
    );
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/joshyorko/plugins.git",
        ],
    );
    let config_dir = temp.path().join("config");
    std::fs::create_dir(&config_dir).unwrap();
    let key_path = config_dir.join("luna-github-app.pem");
    std::fs::write(&key_path, &keys().pem).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&key_path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    let world = Arc::new(Mutex::new(world));
    let base = serve(world.clone()).await;
    let mut config = json!({
        "listen":"127.0.0.1:8787","database":temp.path().join("state/runs.sqlite"),
        "codex_binary":"/absent-codex-github-fixture","skill_path":temp.path().join("SKILL.md"),
        "repositories":{"test":{"root":repo,"max_finish":"local_candidate"}},
        "profiles":{"default":{"effort":"low"}},
        "limits":{"capacity":2,"repair_attempts":1,"wall_seconds":600},
        "github_app":{"app_id":APP_ID,"installation_id":INSTALLATION,"private_key_path":key_path,
            "api_base":base,"allowed_repositories":[REPO]}
    });
    configure(&mut config, &base);
    let config: Config = serde_json::from_value(config).unwrap();
    if let Some(app) = &config.github_app {
        app.validate().unwrap();
    }
    let server = McpServer {
        factory: Factory::new(config).unwrap(),
        html: Arc::new(String::new()),
    };
    Harness {
        _temp: temp,
        server,
        world,
        key_path,
    }
}
fn validator(name: &str) -> jsonschema::Validator {
    let tool = tool_definitions()
        .into_iter()
        .find(|t| t.name == name)
        .unwrap();
    jsonschema::validator_for(&serde_json::to_value(tool.output_schema.unwrap()).unwrap()).unwrap()
}
fn assert_redacted(h: &Harness, value: &Value) {
    let text = value.to_string();
    assert!(!text.contains(TOKEN_PREFIX), "installation token leaked");
    assert!(!text.contains("eyJ"), "JWT leaked");
    assert!(!text.contains("PRIVATE KEY"), "key material leaked");
    assert!(!text.contains("luna-github-app.pem"), "key path leaked");
    assert!(
        !text.contains(h.key_path.to_str().unwrap()),
        "key path leaked"
    );
}
async fn invoke(h: &Harness, name: &str, args: Value) -> Value {
    let output = h.server.invoke(name, args).await.unwrap();
    let errors: Vec<_> = validator(name)
        .iter_errors(&output)
        .map(|e| e.to_string())
        .collect();
    assert!(errors.is_empty(), "{name}: {}", errors.join("; "));
    assert_redacted(h, &output);
    output
}
async fn inspect(h: &Harness, repository: &str, parent: u64) -> Value {
    invoke(
        h,
        "inspect_factory_issue_graph",
        json!({"repository":repository,"parent":parent}),
    )
    .await
}
fn ids(value: &Value) -> Vec<String> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["id"].as_str().unwrap().to_owned())
        .collect()
}

/// Parent #67 with the recorded sub-issue page, plus synthetic edge cases.
fn parent_world() -> World {
    let mut world = World::default();
    let parent_body = "Mission control parent.\n\n## Acceptance\n- [ ] Graph imports cleanly\n\n## Tasks\n- [ ] #71\n- [ ] other-owner/other-repo#3\n- [ ] #75\n- [x] https://github.com/joshyorko/plugins/pull/68\n- [ ] #69 already a sub-issue\n";
    world
        .issues
        .insert(67, issue(REPO, 67, "Mission control", parent_body, true, 0));
    let mut subs = recorded("recorded_sub_issues_67_page.json")
        .as_array()
        .unwrap()
        .clone();
    subs.push(issue("other/repo", 9, "Foreign child", "", true, 0));
    subs.push(issue(REPO, 72, "Closed child", "", false, 0));
    world.sub_issues.insert(67, subs);
    world.issues.insert(
        71,
        issue(
            REPO,
            71,
            "Task-list child",
            "Body.\n### Acceptance\n- Tests pass\n- Docs updated",
            true,
            1,
        ),
    );
    world.gone.insert(75);
    let recorded69 = recorded("recorded_sub_issues_67_page.json")[0].clone();
    world.blocked_by.insert(71, vec![recorded69]);
    world
}

#[tokio::test]
async fn recorded_shapes_become_revisioned_candidates_and_inspection_writes_nothing() {
    let h = harness(parent_world(), |_, _| {}).await;
    let graph = invoke(&h, "create_factory_graph", json!({
        "repository":"test","objective":"Mission control","acceptance":["Graph imports cleanly"],"non_goals":[],
        "finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":0,"wall_seconds":60,"idempotency_key":"intake"
    })).await;
    let run_id = graph["graph"]["run_id"].as_str().unwrap().to_owned();
    let before = invoke(&h, "get_factory_graph", json!({"run_id":run_id})).await;
    let runs_before = invoke(&h, "list_factory_runs", json!({})).await;

    let result = inspect(&h, REPO, 67).await;
    assert_eq!(result["status"], "complete", "{result}");
    assert_eq!(result["writes"], "none");
    assert_eq!(result["reported_by"], "github");
    assert_eq!(result["provenance"], "github_app");
    // Physical binding: the same opaque identity create_factory_graph reports, never a path.
    assert_eq!(
        result["graph_repository"],
        json!({"alias":"test","identity":graph["graph"]["repository"]["identity"]})
    );
    assert_eq!(
        result["eligibility"],
        json!({"eligible":true,"allowed_by_github_app":true,"approved_aliases":["test"],"reasons":[]})
    );
    assert_eq!(
        result["forge_repository"],
        json!({"provider":"github","repository":REPO,"repository_id":REPO_ID,"node_id":REPO_NODE})
    );
    assert_eq!(ids(&result["nodes"]), ["issue-69", "issue-70", "issue-71"]);
    let first = &result["nodes"][0];
    assert_eq!(first["relation"], "sub_issue");
    assert_eq!(first["derivation"], "native");
    assert_eq!(first["provenance"], "github_app");
    // Forge identity is separate from the physical source binding.
    assert_eq!(
        first["forge"],
        json!({"provider":"github","repository":REPO,"number":69,"node_id":"I_kwDORHtYm88AAAABWWXqmw"})
    );
    assert_eq!(first["forge_key"], "github:joshyorko/plugins#69");
    assert_eq!(first["source"]["provider"], "github");
    assert_eq!(
        first["source"]["repository_id"],
        graph["graph"]["repository"]["identity"]
    );
    assert!(first["source"].get("repository").is_none());
    assert!(first["source"].get("repository_node_id").is_none());
    assert_eq!(
        first["source"]["item_id"],
        format!("{REPO_ID}:I_kwDORHtYm88AAAABWWXqmw")
    );
    assert!(
        first["source"]["revision"]
            .as_str()
            .unwrap()
            .starts_with("sha256:")
    );
    assert_eq!(
        first["source"]["display"],
        json!({"number":69,"url":"https://github.com/joshyorko/plugins/issues/69"})
    );
    assert_eq!(result["nodes"][2]["relation"], "task_list");
    assert_eq!(result["nodes"][2]["derivation"], "parsed");
    assert_eq!(result["nodes"][2]["dependencies"], json!(["issue-69"]));
    assert_eq!(
        result["nodes"][2]["dependency_evidence"],
        json!([{"node_id":"issue-69","native":true,"documented":false,"derivation":"native","status":"native"}])
    );
    assert_eq!(
        result["parent_issue"]["forge"],
        json!({"provider":"github","repository":REPO,"number":67,"node_id":"I_kwDOfixture00067"})
    );
    assert_eq!(result["parent_issue"]["provenance"], "github_app");
    let cross = &result["cross_repository"];
    assert!(cross.as_array().unwrap().contains(
        &json!({"repository":"other/repo","number":9,"relation":"sub_issue","derivation":"native","provenance":"github_app","imported":false})
    ));
    assert!(cross.as_array().unwrap().contains(
        &json!({"repository":"other-owner/other-repo","number":3,"relation":"task_list","derivation":"parsed","provenance":"github_app","imported":false})
    ));
    let omitted = result["omitted"].as_array().unwrap();
    for expected in [
        json!({"number":72,"reason":"closed_on_github"}),
        json!({"number":75,"reason":"issue_unavailable"}),
        json!({"number":68,"reason":"pull_request_not_imported"}),
    ] {
        assert!(
            omitted.contains(&expected),
            "{expected} missing from {omitted:?}"
        );
    }
    let acceptance = result["acceptance_candidates"].as_array().unwrap();
    assert!(
        acceptance
            .iter()
            .all(|a| a["label"] == "derived_from_issue")
    );
    assert!(
        acceptance
            .iter()
            .all(|a| a["derivation"] == "parsed" && a["provenance"] == "github_app")
    );
    assert!(acceptance.contains(
        &json!({"id":"issue-67:acceptance-1","node_id":null,"text":"Graph imports cleanly",
        "label":"derived_from_issue","derivation":"parsed","provenance":"github_app"})
    ));
    assert!(
        acceptance
            .iter()
            .any(|a| a["node_id"] == "issue-71" && a["text"] == "Tests pass")
    );
    assert_eq!(result["import"]["ready"], true);
    assert_eq!(
        result["import"]["confirmation"],
        "explicit_user_confirmation_required"
    );
    assert!(
        result["import"]["decisions_needed"]
            .as_array()
            .unwrap()
            .contains(&json!("confirm_parsed_task_list_nodes"))
    );
    assert!(
        result["truncated"]
            .as_object()
            .unwrap()
            .values()
            .all(|v| v == false)
    );
    assert!(result["cycles"].as_array().unwrap().is_empty());

    // Least privilege on the wire: JWT only for installation endpoints, one
    // single-repository read-only token for every read, nothing in query strings.
    let hits = h.hits();
    for hit in &hits {
        let jwt = hit.credential.starts_with("Bearer eyJ");
        let installation =
            hit.path.ends_with("/installation") || hit.path.ends_with("/access_tokens");
        assert_eq!(jwt, installation, "{hit:?}");
        assert!(!hit.query.contains("token") && !hit.query.contains("eyJ"));
        assert_eq!(
            hit.method == "GET",
            !hit.path.ends_with("/access_tokens"),
            "{hit:?}"
        );
    }
    let mints = h.mints();
    assert_eq!(mints.len(), 1);
    assert_eq!(
        mints[0].body,
        json!({"repositories":["plugins"],"permissions":{"issues":"read","metadata":"read"}})
    );

    let after = invoke(&h, "get_factory_graph", json!({"run_id":run_id})).await;
    assert_eq!(before, after, "inspection must not write to the ledger");
    assert_eq!(
        runs_before,
        invoke(&h, "list_factory_runs", json!({})).await
    );
}

#[tokio::test]
async fn app_jwt_is_rs256_backdated_and_valid_for_at_most_ten_minutes() {
    let h = harness(parent_world(), |_, _| {}).await;
    let started = now();
    inspect(&h, REPO, 67).await;
    let world = h.world.lock().unwrap();
    assert!(!world.jwt_claims.is_empty());
    for (claims, header) in world.jwt_claims.iter().zip(&world.jwt_headers) {
        assert_eq!(header["alg"], "RS256");
        assert_eq!(header["typ"], "JWT");
        assert_eq!(claims["iss"], APP_ID.to_string());
        let iat = claims["iat"].as_u64().unwrap();
        let exp = claims["exp"].as_u64().unwrap();
        assert!(
            iat <= started - 55 && iat + 70 >= started,
            "iat {iat} vs {started}"
        );
        assert!(exp > now() && exp - iat <= 600, "exp {exp} iat {iat}");
        assert_eq!(claims.as_object().unwrap().len(), 3, "no extra claims");
    }
}

#[tokio::test]
async fn installation_tokens_are_cached_per_repository_and_scope_until_near_expiry() {
    let h = harness(parent_world(), |_, _| {}).await;
    inspect(&h, REPO, 67).await;
    inspect(&h, "JoshYorko/Plugins", 67).await;
    assert_eq!(
        h.mints().len(),
        1,
        "case-insensitive repository key reuses the token"
    );
    assert_eq!(
        h.hits()
            .iter()
            .filter(|hit| hit.path.ends_with("/installation"))
            .count(),
        1
    );

    let mut short = parent_world();
    short.token_ttl = 120; // inside the refresh margin: never cached
    let h = harness(short, |_, _| {}).await;
    inspect(&h, REPO, 67).await;
    inspect(&h, REPO, 67).await;
    assert_eq!(h.mints().len(), 2);
}

#[tokio::test]
async fn unlisted_or_unapproved_repositories_are_refused_before_any_network_call() {
    let h = harness(parent_world(), |config, _| {
        config["github_app"]["allowed_repositories"] = json!([REPO, "joshyorko/elsewhere"]);
    })
    .await;
    let foreign = inspect(&h, "other/repo", 1).await;
    assert_eq!(foreign["status"], "ineligible");
    assert_eq!(foreign["reason"], "repository_not_in_github_app_allowlist");
    assert_eq!(foreign["eligibility"]["allowed_by_github_app"], false);
    let unapproved = inspect(&h, "joshyorko/elsewhere", 1).await;
    assert_eq!(unapproved["status"], "ineligible");
    assert_eq!(unapproved["reason"], "repository_not_approved_in_luna");
    assert_eq!(unapproved["eligibility"]["approved_aliases"], json!([]));
    assert!(h.hits().is_empty(), "no request may precede eligibility");
    assert_eq!(h.server.factory.github_reader().request_count(), 0);
    assert!(
        h.server
            .invoke(
                "inspect_factory_issue_graph",
                json!({"repository":"../etc","parent":1})
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn sub_issue_pagination_and_candidate_nodes_are_bounded() {
    let mut world = World::default();
    world
        .issues
        .insert(1, issue(REPO, 1, "Large parent", "", true, 0));
    world.sub_issues.insert(
        1,
        (100..160)
            .map(|n| issue(REPO, n, &format!("Child {n}"), "", true, 0))
            .collect(),
    );
    let h = harness(world, |_, _| {}).await;
    let result = inspect(&h, REPO, 1).await;
    assert_eq!(result["nodes"].as_array().unwrap().len(), 32);
    assert_eq!(result["status"], "incomplete");
    assert_eq!(result["truncated"]["nodes"], true);
    assert_eq!(result["truncated"]["sub_issues"], true);
    assert!(
        result["import"]["decisions_needed"]
            .as_array()
            .unwrap()
            .contains(&json!("results_incomplete"))
    );
    let pages: Vec<_> = h
        .hits()
        .into_iter()
        .filter(|hit| hit.path.ends_with("/sub_issues"))
        .map(|hit| hit.query)
        .collect();
    assert_eq!(pages, ["per_page=25&page=1", "per_page=25&page=2"]);
    assert!(
        result["omitted"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|o| o["reason"] == "node_limit")
            .count()
            >= 18
    );
}

#[tokio::test]
async fn cycles_are_rejected_and_foreign_or_closed_dependencies_are_not_imported() {
    let mut world = World::default();
    world
        .issues
        .insert(10, issue(REPO, 10, "Cyclic parent", "", true, 0));
    world.sub_issues.insert(
        10,
        vec![
            issue(REPO, 11, "A", "", true, 1),
            issue(REPO, 12, "B", "", true, 1),
            issue(REPO, 13, "C", "", true, 2),
        ],
    );
    world
        .blocked_by
        .insert(11, vec![issue(REPO, 12, "B", "", true, 1)]);
    world
        .blocked_by
        .insert(12, vec![issue(REPO, 11, "A", "", true, 1)]);
    world.blocked_by.insert(
        13,
        vec![
            issue(REPO, 14, "Closed blocker", "", false, 0),
            issue("other/repo", 1, "Foreign blocker", "", true, 0),
        ],
    );
    let h = harness(world, |_, _| {}).await;
    let result = inspect(&h, REPO, 10).await;
    assert_eq!(
        result["cycles"],
        json!([{"node_ids":["issue-11","issue-12"],"status":"rejected"}])
    );
    assert_eq!(result["import"]["ready"], false);
    assert_eq!(result["import"]["blockers"], json!(["dependency_cycle"]));
    assert_eq!(
        result["external_dependencies"],
        json!([{"node_id":"issue-13","blocked_by":14,"github_state":"closed","imported":false,
            "derivation":"native","provenance":"github_app","status":"native"}])
    );
    assert!(result["cross_repository"].as_array().unwrap().contains(
        &json!({"repository":"other/repo","number":1,"relation":"blocked_by","derivation":"native","provenance":"github_app","imported":false})
    ));
    assert_eq!(result["nodes"][2]["dependencies"], json!([]));
}

async fn import_graph(h: &Harness, inspection: &Value) -> (String, Value) {
    let graph = invoke(h, "create_factory_graph", json!({
        "repository":"test","objective":"Mission control","acceptance":["Graph imports cleanly"],"non_goals":[],
        "finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":0,"wall_seconds":60,"idempotency_key":"github-import"
    })).await;
    let run_id = graph["graph"]["run_id"].as_str().unwrap().to_owned();
    let identity = graph["graph"]["repository"]["identity"].clone();
    // Intake sources are already bound to the graph identity: they are copied unchanged.
    assert_eq!(inspection["graph_repository"]["identity"], identity);
    let nodes: Vec<Value> = inspection["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| {
            json!({"id":n["id"],"title":n["title"],"criterion_ids":["A1"],"dependencies":n["dependencies"],
                "source":n["source"]})
        })
        .collect();
    let change = json!({"kind":"import_candidates","nodes":nodes});
    let proposed = invoke(h, "propose_factory_change", json!({"run_id":run_id,"expected_revision":graph["graph"]["revision"],"idempotency_key":"import-67","change":change})).await;
    let applied = invoke(h, "apply_factory_change", json!({"run_id":run_id,"change_id":proposed["proposal"]["id"],"expected_revision":proposed["graph"]["revision"]})).await;
    (
        run_id,
        json!({"change":change,"proposed":proposed,"applied":applied,"identity":identity,
            "created_revision":graph["graph"]["revision"]}),
    )
}

#[tokio::test]
async fn revisions_follow_issue_fields_and_reimport_is_idempotent_and_source_bound() {
    let h = harness(parent_world(), |_, _| {}).await;
    let first = inspect(&h, REPO, 67).await;
    let unchanged = inspect(&h, REPO, 67).await;
    assert_eq!(first["nodes"], unchanged["nodes"]);
    h.world.lock().unwrap().issues.get_mut(&71).unwrap()["title"] =
        json!("Task-list child, retitled");
    let changed = inspect(&h, REPO, 67).await;
    assert_ne!(
        first["nodes"][2]["source"]["revision"],
        changed["nodes"][2]["source"]["revision"]
    );
    assert_eq!(
        first["nodes"][0]["source"]["revision"],
        changed["nodes"][0]["source"]["revision"]
    );
    assert_eq!(
        first["nodes"][2]["source"]["item_id"],
        changed["nodes"][2]["source"]["item_id"]
    );

    let (run_id, imported) = import_graph(&h, &first).await;
    let graph = &imported["applied"]["graph"];
    let node = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "issue-69")
        .unwrap();
    assert_eq!(node["state"], "candidate");
    assert_eq!(node["source"]["repository_id"], imported["identity"]);
    // An identical retry replays the recorded proposal without another event.
    let replay = invoke(&h, "propose_factory_change", json!({"run_id":run_id,"expected_revision":imported["created_revision"],"idempotency_key":"import-67","change":imported["change"]})).await;
    assert_eq!(
        replay["proposal"]["id"],
        imported["proposed"]["proposal"]["id"]
    );
    // Re-importing the same GitHub item under a new key, even at a new revision, is refused.
    let mut again = imported["change"].clone();
    again["nodes"] = json!([{
        "id":"issue-71-again","title":"Retitled","criterion_ids":["A1"],"dependencies":[],
        "source":{"provider":"github","repository_id":imported["identity"],"item_id":changed["nodes"][2]["source"]["item_id"],"revision":changed["nodes"][2]["source"]["revision"]}
    }]);
    let error = h.server.invoke("propose_factory_change", json!({"run_id":run_id,"expected_revision":graph["revision"],"idempotency_key":"import-67-again","change":again})).await.unwrap_err();
    assert!(
        error.to_string().contains("duplicate_graph_source"),
        "{error}"
    );
    // A source bound to anything other than the graph identity is foreign.
    again["nodes"][0]["source"]["repository_id"] = json!(REPO);
    let error = h.server.invoke("propose_factory_change", json!({"run_id":run_id,"expected_revision":graph["revision"],"idempotency_key":"import-67-foreign","change":again})).await.unwrap_err();
    assert!(
        error.to_string().contains("foreign_graph_source"),
        "{error}"
    );
}

#[tokio::test]
async fn rate_limits_are_structured_and_suppress_requests_until_retry() {
    let mut world = parent_world();
    let reset = now() + 120;
    world.rate_limited = Some(reset);
    let h = harness(world, |_, _| {}).await;
    let result = inspect(&h, REPO, 67).await;
    assert_eq!(result["status"], "unavailable");
    assert_eq!(result["reason"], "rate_limited");
    assert_eq!(result["retry_at"], reset);
    let hits = h.hits().len();
    assert_eq!(hits, 1);
    let again = inspect(&h, REPO, 67).await;
    assert_eq!(again["reason"], "rate_limited");
    assert_eq!(h.hits().len(), hits, "no request before retry_at");
}

#[tokio::test]
async fn unconfigured_unreachable_and_unsafe_key_states_report_clear_reasons() {
    // Unconfigured: disabled by default.
    let h = harness(parent_world(), |config, _| {
        config.as_object_mut().unwrap().remove("github_app");
    })
    .await;
    let capabilities = invoke(&h, "get_factory_capabilities", json!({})).await;
    assert_eq!(
        capabilities["github"],
        json!({"configured":false,"reason":"github_app_not_configured"})
    );
    let result = inspect(&h, REPO, 67).await;
    assert_eq!(
        (result["status"].clone(), result["reason"].clone()),
        (json!("unavailable"), json!("github_app_not_configured"))
    );
    let graph = invoke(&h, "create_factory_graph", json!({
        "repository":"test","objective":"x","acceptance":["y"],"non_goals":[],"finish":"local_candidate",
        "profile":"default","capacity":1,"repair_attempts":0,"wall_seconds":60,"idempotency_key":"k"
    })).await;
    let delivery = invoke(
        &h,
        "read_factory_delivery",
        json!({"run_id":graph["graph"]["run_id"]}),
    )
    .await;
    assert_eq!(delivery["available"], false);
    assert_eq!(delivery["reason"], "github_app_not_configured");
    assert!(h.hits().is_empty());

    // Unreachable: configured, but nothing listens.
    let closed = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap()
    };
    let h = harness(parent_world(), |config, _| {
        config["github_app"]["api_base"] = json!(format!("http://{closed}"));
    })
    .await;
    let capabilities = invoke(&h, "get_factory_capabilities", json!({})).await;
    assert_eq!(capabilities["github"]["configured"], true);
    let result = inspect(&h, REPO, 67).await;
    assert_eq!(result["status"], "unavailable");
    assert_eq!(result["reason"], "github_unreachable");

    // Group- or world-readable keys are refused without any request.
    let h = harness(parent_world(), |_, _| {}).await;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&h.key_path, std::fs::Permissions::from_mode(0o644)).unwrap();
        let capabilities = invoke(&h, "get_factory_capabilities", json!({})).await;
        assert_eq!(
            capabilities["github"],
            json!({"configured":false,"reason":"github_app_key_permissions_too_open"})
        );
        let result = inspect(&h, REPO, 67).await;
        assert_eq!(result["reason"], "github_app_key_permissions_too_open");
        assert!(h.hits().is_empty());
    }

    // A different installation than the operator pinned is refused before minting.
    let mut world = parent_world();
    world.installation_id = 42;
    let h = harness(world, |_, _| {}).await;
    let result = inspect(&h, REPO, 67).await;
    assert_eq!(result["reason"], "github_installation_mismatch");
    assert!(h.mints().is_empty());

    // An installation granted any write permission is refused before minting.
    let mut world = parent_world();
    world.installation_permissions = json!({"issues":"read","metadata":"read","contents":"write"});
    let h = harness(world, |_, _| {}).await;
    let result = inspect(&h, REPO, 67).await;
    assert_eq!(result["reason"], "github_app_not_read_only");
    assert!(h.mints().is_empty());

    // A token broader than requested is discarded.
    let mut world = parent_world();
    world.token_permissions = Some(json!({"issues":"read","metadata":"read","contents":"read"}));
    let h = harness(world, |_, _| {}).await;
    let result = inspect(&h, REPO, 67).await;
    assert_eq!(result["reason"], "github_token_scope_mismatch");
    assert_eq!(
        h.hits()
            .iter()
            .filter(|hit| hit.path.contains("/issues/"))
            .count(),
        0
    );
}

fn delivery_world() -> World {
    let mut world = parent_world();
    let mut pr = recorded("recorded_pr68_rollup.json");
    pr["state"] = json!("OPEN");
    pr["reviewDecision"] = json!("APPROVED");
    world.graphql.insert(
        "I_kwDORHtYm88AAAABWWXqmw".into(),
        json!({"__typename":"Issue","number":69,"state":"OPEN",
            "repository":{"databaseId":REPO_ID,"nameWithOwner":REPO},
            "closedByPullRequestsReferences":{"totalCount":1,"nodes":[pr]}}),
    );
    world.graphql.insert(
        "I_kwDORHtYm88AAAABWWXrKQ".into(),
        json!({"__typename":"Issue","number":70,"state":"OPEN",
            "repository":{"databaseId":999,"nameWithOwner":"someone/else"},
            "closedByPullRequestsReferences":{"totalCount":0,"nodes":[]}}),
    );
    world
}
fn stable_presentation(run: &Value) -> Value {
    let p = &run["presentation"];
    json!({"revision":p["revision"],"primary_action":p["primary_action"],"actions":p["actions"],
        "criteria":p["criteria"],"result":p["result"],"owner":p["owner"],"workers":p["workers"],
        "claim":p["claim"],"deliverable":p["deliverable"]})
}
fn stable_run(run: &Value) -> Value {
    json!({"state":run["state"],"control":run["control"],"presentation":stable_presentation(run),
        "pending_decision":run["pending_decision"],"blocker":run["blocker"],"delta":run["delta"],
        "remaining_gap":run["remaining_gap"],"updated_at":run["updated_at"],"receipts":run["receipts"],
        "claim_held":run["claim_held"]})
}

#[tokio::test]
async fn github_success_never_changes_proof_presentation_actions_or_attention() {
    let h = harness(delivery_world(), |_, _| {}).await;
    let inspection = inspect(&h, REPO, 67).await;
    let (run_id, _) = import_graph(&h, &inspection).await;
    let run_before = invoke(&h, "get_factory_run", json!({"run_id":run_id})).await;
    let graph_before = invoke(&h, "get_factory_graph", json!({"run_id":run_id})).await;
    let workbench_before = invoke(&h, "refresh_factory", json!({"run_id":run_id})).await;

    let delivery = invoke(&h, "read_factory_delivery", json!({"run_id":run_id})).await;
    assert_eq!(delivery["available"], true, "{delivery}");
    assert_eq!(delivery["proof"], "none");
    assert_eq!(delivery["merge_capability"], "none");
    assert_eq!(delivery["reported_by"], "github");
    assert_eq!(delivery["provenance"], "github_app");
    // Two nodes could not be read, so the call as a whole is not complete.
    assert_eq!(delivery["complete"], false);
    let node = |id: &str| {
        delivery["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["node_id"] == id)
            .unwrap()
            .clone()
    };
    let observed = node("issue-69");
    assert_eq!(observed["status"], "observed");
    assert_eq!(observed["freshness"], "fresh");
    assert_eq!(observed["reported_by"], "github");
    assert_eq!(observed["provenance"], "github_app");
    assert_eq!(observed["complete"], true, "{observed}");
    assert!(observed["observed_at"].as_u64().unwrap() >= now() - 5);
    let pr = &observed["pull_requests"][0];
    assert_eq!(pr["number"], 68);
    assert_eq!(pr["url"], "https://github.com/joshyorko/plugins/pull/68");
    assert_eq!(pr["state"], "open");
    assert_eq!(pr["draft"], false);
    assert_eq!(pr["head_sha"], "96c84332c94e21c6ac3af1590cb805993ac2ceda");
    assert_eq!(pr["review_decision"], "approved");
    assert_eq!(
        pr["diff"],
        json!({"files_changed":54,"additions":2451,"deletions":385})
    );
    assert_eq!(pr["checks"]["rollup"], "success");
    assert_eq!(pr["checks"]["runs"].as_array().unwrap().len(), 4);
    assert!(
        pr["checks"]["runs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["conclusion"] == "success" && r["started_at"].is_u64())
    );
    assert_eq!(pr["ready_for_review"], true);
    assert_eq!(pr["merge_authority"], false);
    assert_eq!(pr["reported_by"], "github");
    assert_eq!(pr["provenance"], "github_app");
    // A node whose GitHub item belongs to another repository is not reported as ours.
    assert_eq!(node("issue-70")["status"], "unavailable");
    assert_eq!(node("issue-70")["reason"], "source_repository_mismatch");
    assert_eq!(node("issue-70")["complete"], false);
    assert_eq!(node("issue-71")["reason"], "issue_unavailable");
    let mint = h
        .mints()
        .into_iter()
        .find(|m| m.body["permissions"]["checks"] == "read")
        .unwrap();
    assert_eq!(
        mint.body,
        json!({"repositories":["plugins"],"permissions":{"checks":"read","issues":"read","metadata":"read","pull_requests":"read","statuses":"read"}})
    );

    // GitHub "success" did not touch Luna proof, result, actions or attention.
    let run_after = invoke(&h, "get_factory_run", json!({"run_id":run_id})).await;
    assert_eq!(stable_run(&run_before), stable_run(&run_after));
    assert!(
        run_after["control"]["criteria"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["status"] == "unproved")
    );
    assert_eq!(run_after["presentation"]["criteria"]["proven"], 0);
    assert_ne!(
        run_after["presentation"]["result"]["kind"],
        "finished_verified"
    );
    assert!(run_after["pending_decision"].is_null());
    assert_eq!(
        graph_before,
        invoke(&h, "get_factory_graph", json!({"run_id":run_id})).await
    );
    let workbench_after = invoke(&h, "refresh_factory", json!({"run_id":run_id})).await;
    assert_eq!(
        stable_run(&workbench_before["selected_run"]),
        stable_run(&workbench_after["selected_run"])
    );

    // Bounded polling: a second read inside the minimum interval makes no request.
    let graphql_hits = || h.hits().iter().filter(|hit| hit.path == "/graphql").count();
    let before = graphql_hits();
    let cached = invoke(&h, "read_factory_delivery", json!({"run_id":run_id})).await;
    assert_eq!(graphql_hits(), before);
    let cached_node = cached["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["node_id"] == "issue-69")
        .unwrap()
        .clone();
    assert_eq!(cached_node["freshness"], "cached");
    assert_eq!(cached_node["observed_at"], observed["observed_at"]);

    // After the interval, a rate-limited refresh keeps the old data, marked stale.
    h.server
        .factory
        .github_reader()
        .expire_delivery_cache()
        .await;
    let reset = now() + 300;
    h.world.lock().unwrap().graphql_rate_limited = Some(reset);
    let limited = invoke(&h, "read_factory_delivery", json!({"run_id":run_id})).await;
    assert_eq!(limited["available"], false);
    assert_eq!(limited["reason"], "rate_limited");
    assert_eq!(limited["retry_at"], reset);
    let stale = limited["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["node_id"] == "issue-69")
        .unwrap()
        .clone();
    assert_eq!(stale["freshness"], "stale");
    assert_eq!(
        stale["complete"], false,
        "stale data is never a complete read"
    );
    assert_eq!(limited["complete"], false);
    assert_eq!(stale["observed_at"], observed["observed_at"]);
    assert_eq!(stale["pull_requests"][0]["ready_for_review"], true);
    let hits = h.hits().len();
    invoke(&h, "read_factory_delivery", json!({"run_id":run_id})).await;
    assert_eq!(
        h.hits().len(),
        hits,
        "rate limit suppresses further requests"
    );
    assert_eq!(
        stable_run(&run_before),
        stable_run(&invoke(&h, "get_factory_run", json!({"run_id":run_id})).await)
    );
}

#[tokio::test]
async fn delivery_without_github_sources_or_with_failing_checks_stays_display_only() {
    let mut world = delivery_world();
    let mut pr = recorded("recorded_pr68_rollup.json");
    pr["state"] = json!("OPEN");
    pr["isDraft"] = json!(true);
    pr["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["state"] = json!("FAILURE");
    pr["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"][0]["conclusion"] =
        json!("FAILURE");
    pr["commits"]["nodes"][0]["commit"]["statusCheckRollup"]["contexts"]["nodes"][0]["title"] =
        json!("1 test failed: bearer abc123");
    world.graphql.get_mut("I_kwDORHtYm88AAAABWWXqmw").unwrap()["closedByPullRequestsReferences"]
        ["nodes"] = json!([pr]);
    let h = harness(world, |_, _| {}).await;
    let empty = invoke(&h, "create_factory_graph", json!({
        "repository":"test","objective":"x","acceptance":["y"],"non_goals":[],"finish":"local_candidate",
        "profile":"default","capacity":1,"repair_attempts":0,"wall_seconds":60,"idempotency_key":"empty"
    })).await;
    let none = invoke(
        &h,
        "read_factory_delivery",
        json!({"run_id":empty["graph"]["run_id"]}),
    )
    .await;
    assert_eq!(none["reason"], "no_github_sources");
    assert!(h.hits().is_empty());

    let inspection = inspect(&h, REPO, 67).await;
    let (run_id, _) = import_graph(&h, &inspection).await;
    let before = invoke(&h, "get_factory_run", json!({"run_id":run_id})).await;
    let delivery = invoke(&h, "read_factory_delivery", json!({"run_id":run_id})).await;
    let pr = &delivery["nodes"][0]["pull_requests"][0];
    assert_eq!(pr["draft"], true);
    assert_eq!(pr["checks"]["rollup"], "failure");
    assert_eq!(pr["ready_for_review"], false);
    // Failing-check output text passes the same redaction as other summaries.
    assert!(pr["checks"]["runs"][0]["summary"].is_null());
    assert_eq!(
        stable_run(&before),
        stable_run(&invoke(&h, "get_factory_run", json!({"run_id":run_id})).await)
    );
    assert!(before["pending_decision"].is_null());
}

#[tokio::test]
async fn delivery_polling_is_bounded_per_call_and_per_node() {
    let mut world = World::default();
    world
        .issues
        .insert(2, issue(REPO, 2, "Wide parent", "", true, 0));
    world.sub_issues.insert(
        2,
        (200..220)
            .map(|n| issue(REPO, n, &format!("Child {n}"), "", true, 0))
            .collect(),
    );
    let h = harness(world, |_, _| {}).await;
    let inspection = inspect(&h, REPO, 2).await;
    assert_eq!(inspection["nodes"].as_array().unwrap().len(), 20);
    let (run_id, _) = import_graph(&h, &inspection).await;
    let graphql = || h.hits().iter().filter(|hit| hit.path == "/graphql").count();
    let first = invoke(&h, "read_factory_delivery", json!({"run_id":run_id})).await;
    assert_eq!(graphql(), 16, "at most 16 node fetches per call");
    let statuses: Vec<_> = first["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|n| n["status"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(statuses.iter().filter(|s| *s == "deferred").count(), 4);
    assert!(
        first["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|n| n["status"] == "deferred")
            .all(|n| n["reason"] == "polling_budget_deferred")
    );
    // Failed and deferred nodes obey the same per-node minimum interval.
    invoke(&h, "read_factory_delivery", json!({"run_id":run_id})).await;
    assert_eq!(graphql(), 20);
    invoke(&h, "read_factory_delivery", json!({"run_id":run_id})).await;
    assert_eq!(graphql(), 20);
}

/// Siblings whose prose documents a dependency GitHub does not record natively.
fn documented_world() -> World {
    let mut world = World::default();
    world
        .issues
        .insert(30, issue(REPO, 30, "Documented parent", "", true, 0));
    world.sub_issues.insert(
        30,
        vec![
            issue(REPO, 31, "Base", "", true, 0),
            issue(
                REPO,
                32,
                "Depends in prose",
                "Depends on #31 and blocked by #99.\n\n## Dependencies\n- other-owner/other-repo#4\n",
                true,
                0,
            ),
            issue(REPO, 33, "Native and documented", "Blocked by #31", true, 1),
        ],
    );
    world
        .blocked_by
        .insert(33, vec![issue(REPO, 31, "Base", "", true, 0)]);
    world
}

#[tokio::test]
async fn documented_dependencies_survive_an_absent_native_record() {
    let h = harness(documented_world(), |_, _| {}).await;
    let result = inspect(&h, REPO, 30).await;
    assert_eq!(result["status"], "complete", "{result}");
    let node = |id: &str| {
        result["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .find(|n| n["id"] == id)
            .unwrap()
            .clone()
    };
    // GitHub reports no blocked_by for #32, but its body documents #31: kept, flagged.
    assert_eq!(node("issue-32")["dependencies"], json!(["issue-31"]));
    assert_eq!(
        node("issue-32")["dependency_evidence"],
        json!([{"node_id":"issue-31","native":false,"documented":true,"derivation":"parsed","status":"documented_not_native"}])
    );
    // Recorded natively and documented: native wins, both are visible.
    assert_eq!(
        node("issue-33")["dependency_evidence"],
        json!([{"node_id":"issue-31","native":true,"documented":true,"derivation":"native","status":"native"}])
    );
    assert!(
        result["external_dependencies"]
            .as_array()
            .unwrap()
            .contains(
                &json!({"node_id":"issue-32","blocked_by":99,"github_state":null,
                "imported":false,"derivation":"parsed","provenance":"github_app","status":"documented_not_native"})
            )
    );
    assert!(
        result["cross_repository"].as_array().unwrap().contains(&json!(
            {"repository":"other-owner/other-repo","number":4,"relation":"documented_dependency","derivation":"parsed","provenance":"github_app","imported":false}
        ))
    );
    assert!(
        result["import"]["decisions_needed"]
            .as_array()
            .unwrap()
            .contains(&json!("confirm_documented_dependencies"))
    );
    // Only #33 had a native record to read; #32's summary said zero, so no request.
    let dependency_reads: Vec<_> = h
        .hits()
        .into_iter()
        .filter(|hit| hit.path.ends_with("/dependencies/blocked_by"))
        .map(|hit| hit.path)
        .collect();
    assert_eq!(dependency_reads.len(), 1, "{dependency_reads:?}");

    // Without the dependencies API, the documented edge still stands and the read is incomplete.
    let mut world = documented_world();
    world.dependencies_unavailable = true;
    let h = harness(world, |_, _| {}).await;
    let result = inspect(&h, REPO, 30).await;
    assert_eq!(result["status"], "incomplete");
    assert_eq!(result["truncated"]["dependencies"], true);
    let node33 = result["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|n| n["id"] == "issue-33")
        .unwrap()
        .clone();
    assert_eq!(node33["dependencies"], json!(["issue-31"]));
    assert_eq!(
        node33["dependency_evidence"][0]["status"],
        "documented_not_native"
    );
}

#[tokio::test]
async fn short_dependency_pages_and_unrecognized_items_are_incomplete() {
    let mut world = World::default();
    world
        .issues
        .insert(40, issue(REPO, 40, "Parent", "", true, 0));
    let mut subs = vec![
        issue(REPO, 41, "A", "", true, 0),
        // GitHub says three blockers, but only one comes back.
        issue(REPO, 42, "B", "", true, 3),
    ];
    subs.push(json!({"number":43,"title":"Missing fields"}));
    world.sub_issues.insert(40, subs);
    world
        .blocked_by
        .insert(42, vec![issue(REPO, 41, "A", "", true, 0)]);
    let h = harness(world, |_, _| {}).await;
    let result = inspect(&h, REPO, 40).await;
    assert_eq!(result["status"], "incomplete", "{result}");
    assert_eq!(result["truncated"]["dependencies"], true);
    assert_eq!(result["truncated"]["unrecognized"], true);
    assert!(
        result["import"]["decisions_needed"]
            .as_array()
            .unwrap()
            .contains(&json!("results_incomplete"))
    );
}

#[tokio::test]
async fn ambiguous_repository_bindings_need_an_explicit_alias_before_any_request() {
    let h = harness(parent_world(), |config, _| {
        let root = PathBuf::from(config["repositories"]["test"]["root"].as_str().unwrap());
        let second = root.parent().unwrap().join("second");
        std::fs::create_dir(&second).unwrap();
        git(&second, &["init", "-q"]);
        git(
            &second,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "--allow-empty",
                "-qm",
                "fixture",
            ],
        );
        git(
            &second,
            &[
                "remote",
                "add",
                "origin",
                "git@github.com:joshyorko/plugins.git",
            ],
        );
        config["repositories"]["second"] = json!({"root":second,"max_finish":"local_candidate"});
    })
    .await;
    let ambiguous = inspect(&h, REPO, 67).await;
    assert_eq!(ambiguous["status"], "ineligible");
    assert_eq!(ambiguous["reason"], "repository_alias_required");
    assert!(ambiguous["graph_repository"].is_null());
    let unbound = invoke(
        &h,
        "inspect_factory_issue_graph",
        json!({"repository":REPO,"parent":67,"repository_alias":"elsewhere"}),
    )
    .await;
    assert_eq!(
        unbound["reason"],
        "repository_alias_not_bound_to_github_repository"
    );
    assert!(h.hits().is_empty(), "no request may precede binding");

    let bound = invoke(
        &h,
        "inspect_factory_issue_graph",
        json!({"repository":REPO,"parent":67,"repository_alias":"second"}),
    )
    .await;
    assert_eq!(bound["status"], "complete", "{bound}");
    assert_eq!(bound["graph_repository"]["alias"], "second");
    let graph = invoke(&h, "create_factory_graph", json!({
        "repository":"second","objective":"x","acceptance":["y"],"non_goals":[],"finish":"local_candidate",
        "profile":"default","capacity":1,"repair_attempts":0,"wall_seconds":60,"idempotency_key":"second"
    })).await;
    assert_eq!(
        bound["nodes"][0]["source"]["repository_id"],
        graph["graph"]["repository"]["identity"]
    );
    assert_ne!(
        bound["graph_repository"]["identity"],
        inspect_alias(&h, "test").await["graph_repository"]["identity"]
    );
}
async fn inspect_alias(h: &Harness, alias: &str) -> Value {
    invoke(
        h,
        "inspect_factory_issue_graph",
        json!({"repository":REPO,"parent":67,"repository_alias":alias}),
    )
    .await
}

#[cfg(unix)]
#[tokio::test]
async fn symlinked_keys_and_key_directories_are_refused_without_any_request() {
    use std::os::unix::fs::PermissionsExt;
    // The configured path is a symlink to a correctly locked key.
    let h = harness(parent_world(), |config, _| {
        let key = PathBuf::from(config["github_app"]["private_key_path"].as_str().unwrap());
        let link = key.with_file_name("linked.pem");
        std::os::unix::fs::symlink(&key, &link).unwrap();
        config["github_app"]["private_key_path"] = json!(link);
    })
    .await;
    let capabilities = invoke(&h, "get_factory_capabilities", json!({})).await;
    assert_eq!(
        capabilities["github"],
        json!({"configured":false,"reason":"github_app_key_symlinked"})
    );
    let result = inspect(&h, REPO, 67).await;
    assert_eq!(result["reason"], "github_app_key_symlinked");
    assert!(h.hits().is_empty());

    // An ancestor directory is a symlink.
    let h = harness(parent_world(), |config, _| {
        let key = PathBuf::from(config["github_app"]["private_key_path"].as_str().unwrap());
        let config_dir = key.parent().unwrap();
        let linked_dir = config_dir.with_file_name("config-link");
        std::os::unix::fs::symlink(config_dir, &linked_dir).unwrap();
        config["github_app"]["private_key_path"] = json!(linked_dir.join(key.file_name().unwrap()));
    })
    .await;
    let result = inspect(&h, REPO, 67).await;
    assert_eq!(result["reason"], "github_app_key_symlinked");
    assert!(h.hits().is_empty());

    // Group-readable (not only world-readable) keys are refused too.
    let h = harness(parent_world(), |_, _| {}).await;
    std::fs::set_permissions(&h.key_path, std::fs::Permissions::from_mode(0o640)).unwrap();
    let result = inspect(&h, REPO, 67).await;
    assert_eq!(result["reason"], "github_app_key_permissions_too_open");
    assert!(h.hits().is_empty());
}

#[tokio::test]
async fn credentials_never_reach_debug_errors_or_tool_arguments() {
    let h = harness(parent_world(), |_, _| {}).await;
    // Mint and use a token so every credential exists in memory.
    assert_eq!(inspect(&h, REPO, 67).await["status"], "complete");
    let token = format!("{TOKEN_PREFIX}0001");
    let pem = String::from_utf8_lossy(&keys().pem).into_owned();
    let debug = format!(
        "{:?} {:?}",
        h.server.factory.github_reader(),
        h.server.factory.config
    );
    for secret in [token.as_str(), "eyJ", "PRIVATE KEY", &pem[40..80]] {
        assert!(!debug.contains(secret), "Debug output leaked {secret}");
    }
    assert!(
        !debug.contains(h.key_path.to_str().unwrap()),
        "key path leaked"
    );
    // The API base and key path are operator configuration only: a tool cannot supply them.
    for extra in [
        json!({"api_base":"https://attacker.example"}),
        json!({"private_key_path":"/etc/shadow"}),
        json!({"token":"x"}),
    ] {
        let mut args = json!({"repository":REPO,"parent":67});
        args.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        let error = h
            .server
            .invoke("inspect_factory_issue_graph", args)
            .await
            .unwrap_err()
            .to_string();
        assert!(!error.contains(&token) && !error.contains("eyJ"), "{error}");
    }
    // Request-level failures carry a stable reason, never GitHub's error body or a credential.
    let mut world = parent_world();
    world.installation_id = 7;
    let h = harness(world, |_, _| {}).await;
    let result = inspect(&h, REPO, 67).await;
    assert_eq!(result["reason"], "github_installation_mismatch");
    assert!(!result.to_string().contains("Not Found"));
}

#[tokio::test]
async fn short_sub_issue_pages_against_the_reported_total_are_incomplete() {
    // GitHub's summary counts three sub-issues, but the only page returns one and no next link.
    let world = || {
        let mut world = World::default();
        let mut parent = issue(REPO, 50, "Parent", "", true, 0);
        parent["sub_issues_summary"]["total"] = json!(3);
        world.issues.insert(50, parent);
        world.sub_issues.insert(
            50,
            vec![issue(REPO, 51, "Only child returned", "", true, 0)],
        );
        world
    };
    let h = harness(world(), |_, _| {}).await;
    let result = inspect(&h, REPO, 50).await;
    assert_eq!(result["status"], "incomplete", "{result}");
    assert_eq!(result["truncated"]["sub_issues"], true);
    assert_eq!(ids(&result["nodes"]), ["issue-51"]);
    assert!(
        result["import"]["decisions_needed"]
            .as_array()
            .unwrap()
            .contains(&json!("results_incomplete"))
    );

    // The same single child with a matching total is a complete read.
    let mut matching = world();
    matching.issues.get_mut(&50).unwrap()["sub_issues_summary"]["total"] = json!(1);
    let h = harness(matching, |_, _| {}).await;
    let result = inspect(&h, REPO, 50).await;
    assert_eq!(result["status"], "complete", "{result}");
    assert_eq!(result["truncated"]["sub_issues"], false);
}

#[tokio::test]
async fn credential_echoing_error_bodies_and_persisted_state_stay_clean() {
    // GitHub error bodies are discarded: only a stable reason is reported, even
    // when the body echoes the bearer credential back.
    let mut world = parent_world();
    world.echo_credential_errors = true;
    let h = harness(world, |_, _| {}).await;
    let result = inspect(&h, REPO, 67).await;
    assert_eq!(result["status"], "unavailable");
    assert_eq!(result["reason"], "github_service_unavailable");
    assert!(!result.to_string().contains("upstream failure"), "{result}");

    // A token that covers more than the one requested repository is discarded
    // before any read.
    let mut world = parent_world();
    world.token_repositories = Some(json!([
        {"id":REPO_ID,"node_id":REPO_NODE,"name":"plugins","full_name":REPO,"private":false},
        {"id":7,"node_id":"R_other","name":"other","full_name":"joshyorko/other","private":true}
    ]));
    let h = harness(world, |_, _| {}).await;
    let result = inspect(&h, REPO, 67).await;
    assert_eq!(result["reason"], "github_token_scope_mismatch");
    assert!(
        h.hits().iter().all(|hit| !hit.path.contains("/issues/")),
        "no read with an over-broad token"
    );

    // Intake, import and delivery leave no credential in the ledger, its
    // receipts or the projections built from it.
    let h = harness(delivery_world(), |_, _| {}).await;
    let inspection = inspect(&h, REPO, 67).await;
    let (run_id, _) = import_graph(&h, &inspection).await;
    let delivery = invoke(&h, "read_factory_delivery", json!({"run_id":run_id})).await;
    assert_eq!(delivery["available"], true, "{delivery}");
    assert!(
        h.mints().len() >= 2,
        "intake and delivery tokens were minted"
    );
    for tool in ["get_factory_run", "get_factory_graph", "refresh_factory"] {
        invoke(&h, tool, json!({"run_id":run_id})).await;
    }
    invoke(&h, "list_factory_runs", json!({})).await;
    invoke(&h, "get_factory_capabilities", json!({})).await;
    let pem = String::from_utf8_lossy(&keys().pem).into_owned();
    let state = h.key_path.parent().unwrap().parent().unwrap().join("state");
    let mut scanned = 0;
    for entry in std::fs::read_dir(&state).unwrap() {
        let path = entry.unwrap().path();
        if !path.is_file() {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let text = String::from_utf8_lossy(&bytes);
        for secret in [TOKEN_PREFIX, "eyJ", "PRIVATE KEY", &pem[40..80]] {
            assert!(!text.contains(secret), "persisted state leaked {secret}");
        }
        assert!(
            !text.contains(h.key_path.to_str().unwrap()),
            "key path persisted"
        );
        scanned += 1;
    }
    assert!(scanned > 0, "the ledger was written");
}

// ----------------------------------------------------------------------------
// Workspaces: pick a GitHub repository or start a new local one.

const WIDGET: &str = "joshyorko/widget";

fn workspace_world(count: u64) -> World {
    let mut world = World {
        installation_permissions: json!({"actions":"read","checks":"read","contents":"read",
            "issues":"read","metadata":"read","pull_requests":"read","statuses":"read"}),
        ..World::default()
    };
    world
        .repositories
        .push(repository(REPO, REPO_ID, Some("Plugins")));
    world
        .repositories
        .push(repository(WIDGET, 7, Some("A widget")));
    world
        .repositories
        .push(repository("other-owner/foreign", 8, None));
    for index in 0..count {
        world.repositories.push(repository(
            &format!("joshyorko/repo-{index:03}"),
            100 + index,
            None,
        ));
    }
    world
}
/// GitHub App plus an enabled workspace root under the harness temp directory.
async fn workspace_harness(
    world: World,
    configure: impl FnOnce(&mut Value, &str),
) -> (Harness, PathBuf) {
    let h = harness(world, |config, base| {
        config["github_app"]["allowed_repositories"] = json!(["joshyorko/*"]);
        let root = PathBuf::from(config["database"].as_str().unwrap())
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("workspaces");
        std::fs::create_dir(&root).unwrap();
        config["workspaces"] = json!({"root":root,"connect_github":true,"create_local":true});
        configure(config, base);
    })
    .await;
    let root = h._temp.path().join("workspaces");
    // Bare repositories stand in for GitHub's git endpoint: <fixtures>/<owner>/<repo>.git.
    let fixtures = h._temp.path().join("git-fixtures");
    for name in ["widget", "gadget"] {
        let work = h._temp.path().join(format!("work-{name}"));
        std::fs::create_dir_all(&work).unwrap();
        git(&work, &["init", "-q", "-b", "main"]);
        std::fs::write(work.join("README.md"), format!("# {name}\n")).unwrap();
        git(&work, &["add", "README.md"]);
        git(
            &work,
            &[
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "-c",
                "core.hooksPath=/dev/null",
                "commit",
                "-qm",
                "fixture",
            ],
        );
        let bare = fixtures.join("joshyorko").join(format!("{name}.git"));
        std::fs::create_dir_all(bare.parent().unwrap()).unwrap();
        git(
            h._temp.path(),
            &[
                "clone",
                "-q",
                "--bare",
                work.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
    }
    // Accepted only in debug builds with a loopback API base (absent when unconfigured).
    let configured = h
        .server
        .factory
        .github_reader()
        .unavailable_reason()
        .is_none();
    assert_eq!(
        h.server
            .factory
            .github_reader()
            .use_fixture_clone_source(fixtures),
        configured
    );
    (h, root)
}
async fn invoke_err(h: &Harness, name: &str, args: Value) -> String {
    let error = h.server.invoke(name, args).await.unwrap_err().to_string();
    assert!(
        !error.contains(TOKEN_PREFIX) && !error.contains('/'),
        "{error}"
    );
    error
}
fn git_out(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{args:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}
fn ledger_rows(h: &Harness, table: &str) -> i64 {
    let connection = rusqlite::Connection::open(h._temp.path().join("state/runs.sqlite")).unwrap();
    connection
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
}
fn graph_request(repository: &str, key: &str) -> Value {
    json!({"repository":repository,"objective":"Plan it","acceptance":["It is planned"],"non_goals":[],
        "finish":"local_candidate","profile":"default","capacity":1,"repair_attempts":0,"wall_seconds":60,
        "idempotency_key":key})
}

#[tokio::test]
async fn installation_repositories_are_listed_paginated_and_marked_connected() {
    let mut world = workspace_world(120);
    world.repositories[1]["description"] = json!(format!("Uses ghp_{} somewhere", "x".repeat(20)));
    world.repositories[3]["description"] = json!("d".repeat(900));
    let (h, _) = workspace_harness(world, |_, _| {}).await;
    let first = invoke(&h, "list_factory_github_repositories", json!({})).await;
    assert_eq!(
        (
            first["available"].clone(),
            first["configured"].clone(),
            first["reason"].clone()
        ),
        (json!(true), json!(true), Value::Null)
    );
    assert_eq!(first["reported_by"], "github");
    assert_eq!(first["provenance"], "github_app");
    let repositories = first["repositories"].as_array().unwrap();
    assert_eq!(repositories.len(), 100);
    assert_eq!(first["next_cursor"], "page:2");
    let by_name = |list: &[Value], name: &str| {
        list.iter()
            .find(|r| r["full_name"] == name)
            .cloned()
            .unwrap()
    };
    // The harness's approved alias `test` has a remote naming joshyorko/plugins.
    let plugins = by_name(repositories, REPO);
    assert_eq!(plugins["connected_alias"], "test");
    assert_eq!(
        (
            plugins["connectable"].clone(),
            plugins["blocked_reason"].clone()
        ),
        (json!(false), Value::Null)
    );
    let widget = by_name(repositories, WIDGET);
    assert_eq!(
        (
            widget["connectable"].clone(),
            widget["connected_alias"].clone()
        ),
        (json!(true), Value::Null)
    );
    assert_eq!(
        widget["description"],
        Value::Null,
        "credential-like text is withheld"
    );
    assert_eq!(widget["default_branch"], "main");
    let foreign = by_name(repositories, "other-owner/foreign");
    assert_eq!(
        (
            foreign["connectable"].clone(),
            foreign["blocked_reason"].clone()
        ),
        (
            json!(false),
            json!("repository_not_in_github_app_allowlist")
        )
    );
    assert_eq!(
        by_name(repositories, "joshyorko/repo-000")["description"]
            .as_str()
            .unwrap()
            .chars()
            .count(),
        280
    );
    let second = invoke(
        &h,
        "list_factory_github_repositories",
        json!({"cursor":first["next_cursor"]}),
    )
    .await;
    assert_eq!(second["repositories"].as_array().unwrap().len(), 23);
    assert_eq!(second["next_cursor"], Value::Null);
    // The listing token is metadata-only and minted once; the pinned installation is used.
    let mints = h.mints();
    assert_eq!(mints.len(), 1);
    assert_eq!(mints[0].body, json!({"permissions":{"metadata":"read"}}));
    let listing: Vec<_> = h
        .hits()
        .into_iter()
        .filter(|hit| hit.path == "/installation/repositories")
        .collect();
    assert_eq!(listing.len(), 2);
    assert!(listing[1].query.contains("page=2") && listing[1].query.contains("per_page=100"));
    assert!(
        h.hits()
            .iter()
            .any(|hit| hit.path == format!("/app/installations/{INSTALLATION}"))
    );
    for bad in [
        "page:1",
        "page:99",
        "https://api.github.com/installation/repositories?page=2",
    ] {
        assert_eq!(
            invoke_err(
                &h,
                "list_factory_github_repositories",
                json!({"cursor":bad})
            )
            .await,
            "invalid_github_repository_cursor"
        );
    }
    // Listing writes nothing.
    assert_eq!(ledger_rows(&h, "workspace_repositories"), 0);
    assert_eq!(ledger_rows(&h, "runs"), 0);
    assert!(
        std::fs::read_dir(h._temp.path().join("workspaces"))
            .unwrap()
            .next()
            .is_none()
    );
}

#[tokio::test]
async fn unpinned_listing_needs_exactly_one_installation_and_contents_read_to_connect() {
    let mut world = workspace_world(0);
    world.extra_installations = 1;
    let (h, _) = workspace_harness(world, |config, _| {
        config["github_app"]
            .as_object_mut()
            .unwrap()
            .remove("installation_id");
    })
    .await;
    let ambiguous = invoke(&h, "list_factory_github_repositories", json!({})).await;
    assert_eq!(
        (ambiguous["available"].clone(), ambiguous["reason"].clone()),
        (json!(false), json!("github_installation_ambiguous"))
    );
    assert!(h.mints().is_empty());

    // Without contents: read nothing is connectable and no clone token is minted.
    let mut world = workspace_world(0);
    world.installation_permissions = json!({"issues":"read","metadata":"read"});
    let (h, root) = workspace_harness(world, |_, _| {}).await;
    let listed = invoke(&h, "list_factory_github_repositories", json!({})).await;
    let widget = listed["repositories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["full_name"] == WIDGET)
        .unwrap()
        .clone();
    assert_eq!(
        (
            widget["connectable"].clone(),
            widget["blocked_reason"].clone()
        ),
        (json!(false), json!("github_contents_permission_missing"))
    );
    assert_eq!(
        invoke_err(
            &h,
            "connect_factory_github_repository",
            json!({"repository":WIDGET,"idempotency_key":"k"})
        )
        .await,
        "github_contents_permission_missing"
    );
    assert!(
        !h.mints()
            .iter()
            .any(|m| m.body["permissions"].get("contents").is_some())
    );
    assert!(std::fs::read_dir(&root).unwrap().next().is_none());

    // contents: read keeps the read-only installation check passing; any write fails it.
    let h = harness(parent_world(), |_, _| {}).await;
    h.world.lock().unwrap().installation_permissions =
        json!({"contents":"read","issues":"read","metadata":"read"});
    assert_eq!(inspect(&h, REPO, 67).await["status"], "complete");
    let (h, _) = workspace_harness(workspace_world(0), |_, _| {}).await;
    h.world.lock().unwrap().installation_permissions =
        json!({"contents":"write","metadata":"read"});
    assert_eq!(
        invoke_err(
            &h,
            "connect_factory_github_repository",
            json!({"repository":WIDGET,"idempotency_key":"k"})
        )
        .await,
        "github_app_not_read_only"
    );
    assert!(h.mints().is_empty());
}

#[tokio::test]
async fn allowlist_and_owner_wildcards_gate_connects_before_any_request() {
    let (h, root) = workspace_harness(workspace_world(0), |config, _| {
        config["github_app"]["allowed_repositories"] = json!([REPO, "JoshYorko/Gadget"]);
    })
    .await;
    for repository in [WIDGET, "other-owner/foreign"] {
        assert_eq!(
            invoke_err(
                &h,
                "connect_factory_github_repository",
                json!({"repository":repository,"idempotency_key":"k"})
            )
            .await,
            "repository_not_in_github_app_allowlist"
        );
    }
    assert!(h.hits().is_empty(), "no request may precede the allowlist");
    assert!(std::fs::read_dir(&root).unwrap().next().is_none());
    for bad in [
        json!({"repository":"../etc","idempotency_key":"k"}),
        json!({"repository":WIDGET,"idempotency_key":""}),
        json!({"repository":WIDGET,"idempotency_key":"k","root":"/tmp"}),
    ] {
        assert!(
            h.server
                .invoke("connect_factory_github_repository", bad.clone())
                .await
                .is_err(),
            "{bad}"
        );
    }
    // An owner wildcard admits that owner's repositories and nobody else's.
    let (h, _) = workspace_harness(workspace_world(0), |_, _| {}).await;
    let connected = invoke(
        &h,
        "connect_factory_github_repository",
        json!({"repository":WIDGET,"idempotency_key":"k"}),
    )
    .await;
    assert_eq!(connected["status"], "connected");
    assert_eq!(
        invoke_err(
            &h,
            "connect_factory_github_repository",
            json!({"repository":"other-owner/foreign","idempotency_key":"k2"})
        )
        .await,
        "repository_not_in_github_app_allowlist"
    );
}

#[tokio::test]
async fn disabled_workspace_flags_answer_typed_reasons_and_do_nothing() {
    let (h, root) = workspace_harness(workspace_world(0), |config, _| {
        config["workspaces"]["connect_github"] = json!(false);
        config["workspaces"]["create_local"] = json!(false);
    })
    .await;
    let capabilities = invoke(&h, "get_factory_capabilities", json!({})).await;
    assert_eq!(
        capabilities["workspaces"],
        json!({"connect_github":false,"create_local":false})
    );
    assert!(!capabilities.to_string().contains(root.to_str().unwrap()));
    let listed = invoke(&h, "list_factory_github_repositories", json!({})).await;
    assert_eq!(
        (
            listed["configured"].clone(),
            listed["available"].clone(),
            listed["reason"].clone()
        ),
        (
            json!(true),
            json!(false),
            json!("github_connect_not_enabled")
        )
    );
    assert_eq!(
        invoke_err(
            &h,
            "connect_factory_github_repository",
            json!({"repository":WIDGET,"idempotency_key":"k"})
        )
        .await,
        "github_connect_not_enabled"
    );
    assert_eq!(
        invoke_err(
            &h,
            "create_factory_local_repository",
            json!({"name":"my-app","idempotency_key":"k"})
        )
        .await,
        "local_create_not_enabled"
    );
    assert!(h.hits().is_empty());
    assert!(std::fs::read_dir(&root).unwrap().next().is_none());
    assert_eq!(ledger_rows(&h, "workspace_repositories"), 0);

    // No workspaces block at all: the same reasons, and capabilities report both off.
    let h = harness(workspace_world(0), |_, _| {}).await;
    assert_eq!(
        invoke(&h, "get_factory_capabilities", json!({})).await["workspaces"],
        json!({"connect_github":false,"create_local":false})
    );
    assert_eq!(
        invoke_err(
            &h,
            "create_factory_local_repository",
            json!({"name":"my-app","idempotency_key":"k"})
        )
        .await,
        "local_create_not_enabled"
    );
    // A disabled connect flag is reported without inspecting an absent App key.
    let (h, _) = workspace_harness(workspace_world(0), |config, _| {
        config.as_object_mut().unwrap().remove("github_app");
        config["workspaces"]["connect_github"] = json!(false);
    })
    .await;
    let listed = invoke(&h, "list_factory_github_repositories", json!({})).await;
    assert_eq!(
        (listed["configured"].clone(), listed["reason"].clone()),
        (json!(false), json!("github_connect_not_enabled"))
    );
    assert_eq!(
        invoke_err(
            &h,
            "connect_factory_github_repository",
            json!({"repository":WIDGET,"idempotency_key":"k"})
        )
        .await,
        "github_connect_not_enabled"
    );
    assert!(h.hits().is_empty());
}

#[tokio::test]
async fn connecting_clones_with_a_narrowed_token_and_leaves_no_credential_behind() {
    let (h, root) = workspace_harness(workspace_world(0), |_, _| {}).await;
    let connected = invoke(
        &h,
        "connect_factory_github_repository",
        json!({"repository":"JoshYorko/Widget","idempotency_key":"connect-1"}),
    )
    .await;
    assert_eq!(connected["status"], "connected");
    assert_eq!(connected["alias"], "widget");
    assert_eq!(connected["full_name"], WIDGET);
    assert_eq!(connected["default_branch"], "main");
    assert_eq!(connected["provenance"], "github_app");
    assert_eq!(connected["repository"]["alias"], "widget");
    let identity = connected["repository"]["identity"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(identity.len(), 64);
    // Exactly one clone token: one repository, contents: read only.
    let mints = h.mints();
    assert_eq!(mints.len(), 1);
    assert_eq!(
        mints[0].body,
        json!({"repositories":["Widget"],"permissions":{"contents":"read"}})
    );
    // The clone is shallow, its remote is the plain HTTPS URL and no credential persists.
    let clone = root.join("joshyorko__widget");
    assert_eq!(
        std::fs::read_to_string(clone.join("README.md")).unwrap(),
        "# widget\n"
    );
    assert_eq!(
        git_out(&clone, &["config", "--get", "remote.origin.url"]),
        "https://github.com/joshyorko/widget.git"
    );
    assert_eq!(
        git_out(&clone, &["rev-parse", "--is-shallow-repository"]),
        "true"
    );
    assert_eq!(git_out(&clone, &["branch", "--show-current"]), "main");
    let git_config = std::fs::read_to_string(clone.join(".git/config")).unwrap();
    assert!(!git_config.contains(TOKEN_PREFIX));
    assert!(!git_config.to_ascii_lowercase().contains("extraheader"));
    assert!(!git_config.to_ascii_lowercase().contains("authorization"));
    let names: Vec<String> = std::fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        names,
        vec!["joshyorko__widget"],
        "no staging directory is left"
    );

    // The alias is approved immediately, with provenance, wherever approved repositories appear.
    let expected = json!({"alias":"widget","max_finish":"local_candidate","provenance":"github_app_installation"});
    let capabilities = invoke(&h, "get_factory_capabilities", json!({})).await;
    assert!(
        capabilities["repositories"]
            .as_array()
            .unwrap()
            .contains(&expected)
    );
    assert_eq!(
        capabilities["workspaces"],
        json!({"connect_github":true,"create_local":true})
    );
    let workbench = invoke(&h, "open_factory", json!({})).await;
    assert!(
        workbench["capabilities"]["repositories"]
            .as_array()
            .unwrap()
            .contains(&expected)
    );
    assert!(!workbench.to_string().contains(root.to_str().unwrap()));
    let listed = invoke(&h, "list_factory_github_repositories", json!({})).await;
    let widget = listed["repositories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["full_name"] == WIDGET)
        .unwrap()
        .clone();
    assert_eq!(widget["connected_alias"], "widget");
    assert_eq!(widget["connectable"], false);

    // Connecting starts nothing: no run, claim or receipt.
    assert_eq!(ledger_rows(&h, "runs"), 0);
    assert_eq!(ledger_rows(&h, "claims"), 0);
    assert_eq!(ledger_rows(&h, "receipts"), 0);
    assert_eq!(
        invoke(&h, "list_factory_runs", json!({})).await["runs"],
        json!([])
    );

    // Exact replay: the same result with no request at all.
    let before = h.hits().len();
    let replay = invoke(
        &h,
        "connect_factory_github_repository",
        json!({"repository":"JoshYorko/Widget","idempotency_key":"connect-1"}),
    )
    .await;
    assert_eq!(replay, connected);
    assert_eq!(h.hits().len(), before);
    let again = invoke(
        &h,
        "connect_factory_github_repository",
        json!({"repository":WIDGET,"idempotency_key":"connect-2"}),
    )
    .await;
    assert_eq!(again["status"], "already_connected");
    assert_eq!(again["repository"], connected["repository"]);
    assert_eq!(h.hits().len(), before);
    assert_eq!(
        invoke_err(
            &h,
            "connect_factory_github_repository",
            json!({"repository":"joshyorko/gadget","idempotency_key":"connect-1"})
        )
        .await,
        "idempotency_conflict"
    );
    // A repository already approved through the operator config is already connected.
    let existing = invoke(
        &h,
        "connect_factory_github_repository",
        json!({"repository":REPO,"idempotency_key":"connect-3"}),
    )
    .await;
    assert_eq!(
        (existing["status"].clone(), existing["alias"].clone()),
        (json!("already_connected"), json!("test"))
    );
    assert_eq!(
        invoke_err(
            &h,
            "connect_factory_github_repository",
            json!({"repository":WIDGET,"idempotency_key":"connect-3"}),
        )
        .await,
        "idempotency_conflict"
    );
    assert_eq!(h.hits().len(), before);

    // The new alias plans like any approved repository, bound to the same identity.
    let graph = invoke(&h, "create_factory_graph", graph_request("widget", "g1")).await;
    assert_eq!(graph["graph"]["repository"]["alias"], "widget");
    assert_eq!(graph["graph"]["repository"]["identity"], identity);
    assert_eq!(graph["graph"]["planning_only"], true);
    assert_eq!(graph["graph"]["claim"]["held"], false);
    assert_eq!(ledger_rows(&h, "claims"), 0);
    assert_eq!(ledger_rows(&h, "workspace_repositories"), 1);
    assert_eq!(ledger_rows(&h, "repository_registrations"), 0);
}

#[tokio::test]
async fn replacing_workspace_root_during_token_mint_keeps_clone_in_pinned_root() {
    let (h, root) = workspace_harness(workspace_world(0), |_, _| {}).await;
    let moved = h._temp.path().join("workspaces-original");
    let elsewhere = h._temp.path().join("replacement");
    std::fs::create_dir(&elsewhere).unwrap();
    let root_for_hook = root.clone();
    let moved_for_hook = moved.clone();
    let elsewhere_for_hook = elsewhere.clone();
    assert!(
        h.server
            .factory
            .github_reader()
            .use_clone_hook(std::sync::Arc::new(move || {
                std::fs::rename(&root_for_hook, &moved_for_hook).unwrap();
                std::os::unix::fs::symlink(&elsewhere_for_hook, &root_for_hook).unwrap();
            }))
    );
    let root_for_restore = root.clone();
    let moved_for_restore = moved.clone();
    assert!(
        h.server
            .factory
            .github_reader()
            .use_clone_completion_hook(std::sync::Arc::new(move || {
                std::fs::remove_file(&root_for_restore).unwrap();
                std::fs::rename(&moved_for_restore, &root_for_restore).unwrap();
            }))
    );

    let connected = invoke(
        &h,
        "connect_factory_github_repository",
        json!({"repository":"joshyorko/widget","idempotency_key":"root-swap"}),
    )
    .await;
    assert_eq!(connected["status"], "connected");
    assert!(root.join("joshyorko__widget/README.md").is_file());
    assert!(!moved.exists());
    assert!(!elsewhere.join("joshyorko__widget").exists());
    assert_eq!(std::fs::read_dir(&elsewhere).unwrap().count(), 0);
}

#[tokio::test]
async fn clone_file_limit_is_hard_and_failed_clone_cleans_staging() {
    let mut world = workspace_world(0);
    world
        .repositories
        .push(repository("joshyorko/oversized", 9, None));
    let (h, root) = workspace_harness(world, |config, _| {
        config["workspaces"]["max_clone_bytes"] = json!(1_048_576_u64);
    })
    .await;
    let work = h._temp.path().join("work-oversized");
    std::fs::create_dir(&work).unwrap();
    git(&work, &["init", "-q", "-b", "main"]);
    let mut state = 0x6d2b_79f5_u32;
    let mut large = vec![0_u8; 2 * 1024 * 1024];
    for byte in &mut large {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *byte = state as u8;
    }
    std::fs::write(work.join("large.bin"), large).unwrap();
    git(&work, &["add", "large.bin"]);
    git(
        &work,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "commit",
            "-qm",
            "large fixture",
        ],
    );
    let fixtures = h._temp.path().join("git-fixtures/joshyorko");
    std::fs::create_dir_all(&fixtures).unwrap();
    git(
        h._temp.path(),
        &[
            "clone",
            "-q",
            "--bare",
            work.to_str().unwrap(),
            fixtures.join("oversized.git").to_str().unwrap(),
        ],
    );

    assert_eq!(
        invoke_err(
            &h,
            "connect_factory_github_repository",
            json!({"repository":"joshyorko/oversized","idempotency_key":"too-large"}),
        )
        .await,
        "clone_too_large"
    );
    assert_eq!(
        std::fs::read_dir(&root).unwrap().count(),
        0,
        "staging is removed"
    );
}

#[tokio::test]
async fn existing_workspace_paths_conflict_unless_they_are_the_same_repository() {
    let (h, root) = workspace_harness(workspace_world(0), |_, _| {}).await;
    h.world
        .lock()
        .unwrap()
        .repositories
        .push(repository("joshyorko/gadget", 9, None));
    // A foreign directory at the target is refused and left untouched.
    let occupied = root.join("joshyorko__widget");
    std::fs::create_dir(&occupied).unwrap();
    std::fs::write(occupied.join("notes.txt"), "mine").unwrap();
    assert_eq!(
        invoke_err(
            &h,
            "connect_factory_github_repository",
            json!({"repository":WIDGET,"idempotency_key":"k1"})
        )
        .await,
        "workspace_path_conflict"
    );
    assert_eq!(
        std::fs::read_to_string(occupied.join("notes.txt")).unwrap(),
        "mine"
    );
    assert!(
        h.mints().is_empty(),
        "no token is minted for a conflicting path"
    );
    // A symlink at the target is refused too.
    std::os::unix::fs::symlink(h._temp.path(), root.join("my-app")).unwrap();
    assert_eq!(
        invoke_err(
            &h,
            "create_factory_local_repository",
            json!({"name":"my-app","idempotency_key":"k2"})
        )
        .await,
        "workspace_path_conflict"
    );
    // A matching origin does not prove Luna created or controls a directory.
    let adopted = root.join("joshyorko__gadget");
    std::fs::create_dir(&adopted).unwrap();
    git(&adopted, &["init", "-q"]);
    git(
        &adopted,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "core.hooksPath=/dev/null",
            "commit",
            "--allow-empty",
            "-qm",
            "existing",
        ],
    );
    git(
        &adopted,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/joshyorko/gadget.git",
        ],
    );
    git(&adopted, &["config", "core.fsmonitor", "true"]);
    std::fs::write(adopted.join(".git/hooks/pre-commit"), "#!/bin/sh\nexit 1\n").unwrap();
    assert_eq!(
        invoke_err(
            &h,
            "connect_factory_github_repository",
            json!({"repository":"joshyorko/gadget","idempotency_key":"k3"}),
        )
        .await,
        "workspace_path_conflict"
    );
    assert!(h.mints().is_empty());
    let capabilities = invoke(&h, "get_factory_capabilities", json!({})).await;
    let aliases: Vec<_> = capabilities["repositories"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["alias"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(aliases, vec!["test"]);
    assert_eq!(ledger_rows(&h, "runs"), 0);
}

#[tokio::test]
async fn local_repositories_are_created_replayed_and_approved_without_github() {
    let (h, root) = workspace_harness(workspace_world(0), |_, _| {}).await;
    let created = invoke(
        &h,
        "create_factory_local_repository",
        json!({"name":"my.app","idempotency_key":"create-1"}),
    )
    .await;
    assert_eq!(created["status"], "created");
    assert_eq!(created["alias"], "my-app");
    assert_eq!(created["repository"]["alias"], "my-app");
    let identity = created["repository"]["identity"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(identity.len(), 64);
    let repo = root.join("my.app");
    assert_eq!(git_out(&repo, &["branch", "--show-current"]), "main");
    assert_eq!(git_out(&repo, &["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(git_out(&repo, &["ls-tree", "-r", "HEAD"]), "");
    assert_eq!(git_out(&repo, &["remote"]), "");
    assert!(
        h.hits().is_empty(),
        "creating a local repository calls no GitHub API"
    );

    let replay = invoke(
        &h,
        "create_factory_local_repository",
        json!({"name":"my.app","idempotency_key":"create-1"}),
    )
    .await;
    assert_eq!(replay, created);
    let again = invoke(
        &h,
        "create_factory_local_repository",
        json!({"name":"my.app","idempotency_key":"create-2"}),
    )
    .await;
    assert_eq!(again["status"], "already_exists");
    assert_eq!(again["repository"], created["repository"]);
    assert_eq!(
        invoke_err(
            &h,
            "create_factory_local_repository",
            json!({"name":"other","idempotency_key":"create-2"}),
        )
        .await,
        "idempotency_conflict"
    );
    assert_eq!(git_out(&repo, &["rev-list", "--count", "HEAD"]), "1");
    assert_eq!(
        invoke_err(
            &h,
            "create_factory_local_repository",
            json!({"name":"other","idempotency_key":"create-1"})
        )
        .await,
        "idempotency_conflict"
    );
    for bad in ["My-App", "-x", ".hidden", "a/b", ""] {
        assert!(
            h.server
                .invoke(
                    "create_factory_local_repository",
                    json!({"name":bad,"idempotency_key":"create-9"})
                )
                .await
                .is_err(),
            "{bad}"
        );
    }
    // An alias already taken (here by the operator's `test`) is refused cleanly.
    assert_eq!(
        invoke_err(
            &h,
            "create_factory_local_repository",
            json!({"name":"test","idempotency_key":"create-3"})
        )
        .await,
        "repository_alias_conflict"
    );
    assert!(!root.join("test").exists());

    let capabilities = invoke(&h, "get_factory_capabilities", json!({})).await;
    assert!(capabilities["repositories"].as_array().unwrap().contains(
        &json!({"alias":"my-app","max_finish":"local_candidate","provenance":"local_create"})
    ));
    let graph = invoke(&h, "create_factory_graph", graph_request("my-app", "g1")).await;
    assert_eq!(graph["graph"]["repository"]["identity"], identity);
    let campaign = invoke(&h, "create_factory_campaign", json!({
        "repository":"my-app","parent":{"forge":{"provider":"local","repository":"local/my-app","number":1},"revision":"r1"},
        "title":"First","objective":"Plan it","acceptance":["It is planned"],"non_goals":[],
        "finish":"local_candidate","profile":"default","idempotency_key":"c1"
    })).await;
    assert_eq!(campaign["campaign"]["repository"]["alias"], "my-app");
    assert_eq!(campaign["campaign"]["repository"]["identity"], identity);
    assert_eq!(ledger_rows(&h, "claims"), 0);
    assert!(h.hits().is_empty());
}

#[tokio::test]
async fn workspace_approval_follows_the_operator_flag_and_survives_restart() {
    let (h, root) = workspace_harness(workspace_world(0), |_, _| {}).await;
    invoke(
        &h,
        "create_factory_local_repository",
        json!({"name":"keep","idempotency_key":"k"}),
    )
    .await;
    let config_path = h._temp.path().join("state/runs.sqlite");
    let mut config = json!({
        "listen":"127.0.0.1:8787","database":config_path,
        "codex_binary":"/absent-codex-github-fixture","skill_path":h._temp.path().join("SKILL.md"),
        "repositories":{},"profiles":{"default":{"effort":"low"}},
        "limits":{"capacity":2,"repair_attempts":1,"wall_seconds":600},
        "workspaces":{"root":root,"connect_github":false,"create_local":true}
    });
    let Harness {
        _temp: temp,
        server,
        ..
    } = h;
    drop(server);
    let restarted = McpServer {
        factory: Factory::new(serde_json::from_value(config.clone()).unwrap()).unwrap(),
        html: Arc::new(String::new()),
    };
    let capabilities = restarted
        .invoke("get_factory_capabilities", json!({}))
        .await
        .unwrap();
    assert_eq!(
        capabilities["repositories"],
        json!([{"alias":"keep","max_finish":"local_candidate","provenance":"local_create"}])
    );
    drop(restarted);
    // Turning the flag off withdraws the approval; the directory is left alone.
    config["workspaces"]["create_local"] = json!(false);
    let revoked = McpServer {
        factory: Factory::new(serde_json::from_value(config).unwrap()).unwrap(),
        html: Arc::new(String::new()),
    };
    assert_eq!(
        revoked
            .invoke("get_factory_capabilities", json!({}))
            .await
            .unwrap()["repositories"],
        json!([])
    );
    assert!(
        revoked
            .invoke("create_factory_graph", graph_request("keep", "g"))
            .await
            .is_err()
    );
    assert!(root.join("keep/.git").is_dir());
    drop(temp);
}
