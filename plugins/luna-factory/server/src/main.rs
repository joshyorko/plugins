use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use luna_factoryd::{
    config::Config,
    lifecycle::Factory,
    native::{NativeClient, validate_luna_route},
    qualification::{self, ExecutionProfile},
    store::repository_subject,
};
use serde_json::json;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

async fn current_provider_identity(
    config: &Config,
) -> Result<Option<qualification::NativeServerIdentity>> {
    if config.native_transport != "existing_daemon" {
        return Ok(None);
    }
    let client =
        NativeClient::connect_existing(&config.codex_binary, config.native_socket.as_deref())
            .await?;
    let identity = client
        .provider_identity()
        .context(qualification::PROVIDER_MISMATCH)?;
    client.shutdown().await?;
    Ok(Some(identity))
}

#[derive(Parser)]
#[command(version, about = "Local Luna Factory MCP runtime")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Serve {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        ui: Option<PathBuf>,
    },
    Doctor {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        probe_native: bool,
    },
    Status {
        #[arg(long)]
        config: PathBuf,
    },
    Subject {
        #[arg(long)]
        root: PathBuf,
    },
    RepositoryRequests {
        #[arg(long)]
        config: PathBuf,
    },
    ApproveRepository {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        request_id: String,
    },
    /// Print the exact execution profile and its fingerprint. Reads no secrets.
    QualificationProfile {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        profile: String,
    },
    /// Operator-only: record a qualification grant for the current exact profile.
    RecordQualification {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        evidence: PathBuf,
        #[arg(long)]
        scope: String,
        #[arg(long, default_value_t = 1)]
        max_capacity: u32,
        #[arg(long)]
        expires_in_days: Option<u64>,
    },
    /// Operator-only: revoke a recorded grant by fingerprint.
    RevokeQualification {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        fingerprint: String,
    },
    /// List recorded grants; evidence content is never echoed.
    Qualifications {
        #[arg(long)]
        config: PathBuf,
    },
    /// Operator-only canary for a `qualification_only` grant. Never reachable via MCP.
    QualificationCanary {
        #[arg(long)]
        config: PathBuf,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        workspace: PathBuf,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Version,
}
#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Version => println!("luna-factoryd {}", env!("CARGO_PKG_VERSION")),
        Command::Subject { root } => println!("{}", repository_subject(&root.canonicalize()?)?),
        Command::RepositoryRequests { config } => {
            let config = Config::load(&config)?;
            let store = luna_factoryd::store::Store::open(&config)?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &store
                        .repository_registrations()?
                        .iter()
                        .map(|r| r.public())
                        .collect::<Vec<_>>()
                )?
            );
        }
        Command::ApproveRepository { config, request_id } => {
            let config = Config::load(&config)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&luna_factoryd::repositories::approve(
                    &config,
                    &request_id
                )?)?
            );
        }
        Command::QualificationProfile { config, profile } => {
            let config = Config::load(&config)?;
            let profile = ExecutionProfile::from_config(&config, &profile)?;
            let provider_identity = current_provider_identity(&config).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &json!({"profile":profile,"fingerprint":profile.fingerprint(),"provider_identity":provider_identity})
                )?
            );
        }
        Command::RecordQualification {
            config,
            profile,
            evidence,
            scope,
            max_capacity,
            expires_in_days,
        } => {
            let config = Config::load(&config)?;
            qualification::ensure_operator_context()?;
            let provider_identity = current_provider_identity(&config).await?;
            let profile = ExecutionProfile::from_config(&config, &profile)?;
            let fingerprint = profile.fingerprint();
            let confirmation = qualification::confirm_operator_action(
                &format!(
                    "Record execution qualification\nprofile: {}\nfingerprint: {}\ntransport: {}\nmodel: {}\neffort: {}\nprovider_identity: {}\nevidence: {}\nscope: {}\nmax_capacity: {}\nexpires_in_days: {:?}",
                    profile.profile,
                    fingerprint,
                    profile.transport,
                    profile.model,
                    profile.effort,
                    serde_json::to_string(&provider_identity)?,
                    evidence.display(),
                    scope,
                    max_capacity,
                    expires_in_days
                ),
                &profile.profile,
            )?;
            let record = qualification::record(
                &config,
                qualification::RecordRequest {
                    alias: &profile,
                    evidence: &evidence,
                    scope: qualification::Scope::parse(&scope)?,
                    max_capacity,
                    expires_in_days,
                    provider_identity,
                },
                &confirmation,
            )?;
            println!("{}", serde_json::to_string_pretty(&record)?);
        }
        Command::RevokeQualification {
            config,
            fingerprint,
        } => {
            let config = Config::load(&config)?;
            qualification::ensure_operator_context()?;
            let profile = qualification::profile_alias(&config, &fingerprint)?;
            let confirmation = qualification::confirm_operator_action(
                &format!(
                    "Revoke execution qualification\nprofile: {profile}\nfingerprint: {fingerprint}"
                ),
                &profile,
            )?;
            println!(
                "{}",
                serde_json::to_string_pretty(&qualification::revoke(
                    &config,
                    &fingerprint,
                    &confirmation
                )?)?
            );
        }
        Command::Qualifications { config } => {
            let config = Config::load(&config)?;
            println!(
                "{}",
                serde_json::to_string_pretty(&qualification::list(&config)?)?
            );
        }
        Command::QualificationCanary {
            config,
            profile,
            workspace,
            output,
        } => {
            let config = Config::load(&config)?;
            // Approved registrations are repositories too; the canary must avoid all of them.
            let effective = luna_factoryd::repositories::effective_config(
                &config,
                &luna_factoryd::store::Store::open(&config)?.repository_registrations()?,
            );
            let preconditions =
                qualification::canary_preconditions(&effective, &profile, &workspace)?;
            if let Some(output) = output {
                ensure!(
                    !output.exists(),
                    "qualification_canary_output_exists; refusing to overwrite"
                );
            }
            eprintln!("{}", serde_json::to_string_pretty(&preconditions)?);
            // Never fake success: no native process is started and no evidence is written
            // until the bounded owner turn, independent hash check and cessation proof exist.
            anyhow::bail!(
                "qualification_canary_not_yet_implemented: preconditions passed; no native process was started and no evidence was written"
            );
        }
        Command::Status { config } => {
            let config = Config::load(&config)?;
            // Status must not run startup reconciliation or contact native Codex.
            let store = luna_factoryd::store::Store::open(&config)?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &store
                        .list(100)?
                        .iter()
                        .map(|run| luna_factoryd::lifecycle::public_run_with_activity(run, &store))
                        .collect::<Result<Vec<_>>>()?
                )?
            );
        }
        Command::Doctor {
            config,
            probe_native,
        } => {
            let config = Config::load(&config)?;
            let version = std::process::Command::new(&config.codex_binary)
                .arg("--version")
                .output()
                .ok()
                .filter(|r| r.status.success())
                .and_then(|r| String::from_utf8(r.stdout).ok())
                .map(|v| v.trim().to_string());
            let skill = config
                .skill_path
                .canonicalize()
                .ok()
                .and_then(|p| std::fs::read(p).ok())
                .map(|v| format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(v)));
            let mut report = json!({"runtime":env!("CARGO_PKG_VERSION"),"codex_version":version,"skill_sha256":skill,"configuration_valid":true,"native_transport":config.native_transport,"inference_calls":0,"effective_route":"unverified","native_probe":"not_requested"});
            if probe_native {
                let connection = if config.native_transport == "existing_daemon" {
                    NativeClient::connect_existing(
                        &config.codex_binary,
                        config.native_socket.as_deref(),
                    )
                    .await
                } else {
                    NativeClient::spawn(
                        &config.codex_binary,
                        &["app-server".into(), "--stdio".into()],
                    )
                    .await
                };
                match connection {
                    Ok(client) => {
                        report["native_probe"] = match client.list_models().await {
                            Ok(catalog) => {
                                json!({"initialize":"passed","catalog_luna_low":validate_luna_route(&catalog,"low").is_ok(),"execution":"not_requested"})
                            }
                            Err(_) => json!("model_list_unavailable"),
                        };
                        client.shutdown().await?;
                    }
                    Err(_) => {
                        report["native_probe"] = json!(
                            "initialize_failed; inspect native Codex locally; no inference or fallback requested"
                        )
                    }
                }
            }
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        Command::Serve { config, ui } => {
            let config = Config::load(&config)?;
            let ui = ui.unwrap_or_else(|| {
                config
                    .skill_path
                    .parent()
                    .and_then(|p| p.parent())
                    .and_then(|p| p.parent())
                    .unwrap_or(std::path::Path::new("."))
                    .join("ui/dist/index.html")
            });
            let html = std::fs::read_to_string(ui).context(
                "Build ui/dist/index.html or supply --ui with the installed app artifact",
            )?;
            ensure!(
                html.len() <= 4 * 1024 * 1024,
                "UI artifact exceeds size limit"
            );
            let listen = config.listen;
            let factory = Factory::new(config)?;
            factory.reconcile_startup().await?;
            let token = CancellationToken::new();
            let router = luna_factoryd::http::router(factory, html, token.child_token());
            let listener = tokio::net::TcpListener::bind(listen).await?;
            eprintln!("Luna Factory listening on {listen}/mcp");
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _ = tokio::signal::ctrl_c().await;
                    token.cancel();
                })
                .await?;
        }
    }
    Ok(())
}
