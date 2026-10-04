use anyhow::{Context, Result, ensure};
use clap::{Parser, Subcommand};
use luna_factoryd::{
    config::Config,
    lifecycle::Factory,
    native::{NativeClient, validate_luna_route},
    store::repository_subject,
};
use serde_json::json;
use std::path::PathBuf;
use tokio_util::sync::CancellationToken;

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
    Version,
}
#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Version => println!("luna-factoryd {}", env!("CARGO_PKG_VERSION")),
        Command::Subject { root } => println!("{}", repository_subject(&root.canonicalize()?)?),
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
                        .map(luna_factoryd::lifecycle::public_run)
                        .collect::<Vec<_>>()
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
                let args = if config.native_transport == "existing_daemon" {
                    let mut args = vec!["app-server".into(), "proxy".into()];
                    if let Some(socket) = &config.native_socket {
                        args.extend(["--sock".into(), socket.to_string_lossy().into_owned()]);
                    }
                    args
                } else {
                    vec!["app-server".into(), "--stdio".into()]
                };
                match NativeClient::spawn(&config.codex_binary, &args).await {
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
