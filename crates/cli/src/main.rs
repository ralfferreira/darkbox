use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use darkbox_contracts::*;
use darkbox_storage::{Db, NewProcedure};
use sha2::{Digest, Sha256};
use tracing_subscriber::EnvFilter;

mod verify;

const DEV_TOKEN: &str = "darkbox-dev-token";
const OTHER_TOKEN: &str = "darkbox-other-token";

#[derive(Parser)]
#[command(name = "darkbox", version, about = "DarkBox assessment platform")]
struct Cli {
    #[arg(
        long,
        env = "DATABASE_URL",
        default_value = "postgres://darkbox:darkbox@127.0.0.1:5433/darkbox"
    )]
    database_url: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Apply database migrations.
    Migrate,
    /// Run the HTTP API.
    Serve {
        #[arg(long, default_value = "127.0.0.1:8787")]
        bind: String,
    },
    /// Run the execution worker.
    Worker {
        #[arg(long)]
        once: bool,
        #[arg(long)]
        assessment: Option<String>,
    },
    /// Seed two workspaces, a synthetic assessment and the procedure catalog.
    Seed {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        lab_url: String,
    },
    /// Load or refresh procedure manifests.
    Procedures {
        #[arg(long, default_value = "procedures")]
        dir: PathBuf,
    },
    /// Delete evidence whose retention window expired.
    Purge,
    /// Run the acceptance harness against a live lab and API.
    Verify {
        #[arg(long, default_value = "http://127.0.0.1:8080")]
        lab_url: String,
        #[arg(long, default_value = "http://127.0.0.1:8787")]
        api_url: String,
        #[arg(long, default_value = DEV_TOKEN)]
        token: String,
        #[arg(long, default_value = "artifacts/acceptance/report.json")]
        output: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_target(false)
        .init();
    let cli = Cli::parse();
    let db = Db::connect(&cli.database_url)
        .await
        .context("connect to PostgreSQL (is docker compose up?)")?;
    match cli.command {
        Command::Migrate => {
            db.migrate().await?;
            println!("migrations applied");
        }
        Command::Serve { bind } => {
            db.migrate().await?;
            let state = darkbox_api::AppState::new(db, darkbox_api::ApiConfig::from_env()).await?;
            darkbox_api::serve(state, &bind).await?;
        }
        Command::Worker { once, assessment } => {
            db.migrate().await?;
            let executor = std::sync::Arc::new(darkbox_execution::Executor::new(
                db,
                darkbox_execution::ExecutorConfig::from_env(),
            )?);
            let filter = assessment
                .map(|raw| raw.parse::<AssessmentId>())
                .transpose()
                .map_err(|_| anyhow::anyhow!("invalid assessment id"))?;
            executor.worker_loop(filter, once).await;
        }
        Command::Seed { lab_url } => {
            db.migrate().await?;
            seed(&db, &lab_url).await?;
        }
        Command::Procedures { dir } => {
            db.migrate().await?;
            let count = sync_procedures(&db, &dir).await?;
            println!("{count} procedure versions loaded");
        }
        Command::Purge => {
            db.migrate().await?;
            let removed = purge(&db).await?;
            println!("{removed} expired evidence objects removed");
        }
        Command::Verify {
            lab_url,
            api_url,
            token,
            output,
        } => {
            db.migrate().await?;
            let report = verify::run(&db, &lab_url, &api_url, &token).await?;
            if let Some(parent) = output.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&output, serde_json::to_vec_pretty(&report)?)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            println!("report written to {}", output.display());
            if report.failed > 0 {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}

fn hash_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

async fn seed(db: &Db, lab_url: &str) -> Result<()> {
    let lab_port = url::Url::parse(lab_url)
        .ok()
        .and_then(|url| url.port_or_known_default())
        .unwrap_or(8080);

    let workspace = match db.workspace_by_name("DarkBox Lab").await? {
        Some(id) => id,
        None => db.create_workspace("DarkBox Lab", 30).await?,
    };
    let other_workspace = match db.workspace_by_name("Other Workspace").await? {
        Some(id) => id,
        None => db.create_workspace("Other Workspace", 30).await?,
    };

    db.seed_token(
        &hash_token(DEV_TOKEN),
        workspace,
        "lab-operator",
        Role::Admin,
    )
    .await?;
    db.seed_token(
        &hash_token(OTHER_TOKEN),
        other_workspace,
        "other-operator",
        Role::Admin,
    )
    .await?;

    let company_id = match db.company_by_name(workspace, "Acme Synthetic").await? {
        Some(id) => id,
        None => {
            db.create_company(
                workspace,
                "Acme Synthetic",
                "Synthetic company used by the bundled laboratory. No real systems.",
            )
            .await?
        }
    };
    let other_company = match db.company_by_name(other_workspace, "Other Co").await? {
        Some(id) => id,
        None => {
            db.create_company(other_workspace, "Other Co", "Isolation test workspace")
                .await?
        }
    };

    let mut policy = ScopePolicy::demo(lab_port);
    policy.include_hosts = vec!["127.0.0.1".into(), "localhost".into()];
    policy.exclude_paths = vec![
        "/admin/forbidden".into(),
        "/app/north/admin/forbidden".into(),
        "/app/south/admin/forbidden".into(),
    ];

    let targets = [
        format!("http://127.0.0.1:{lab_port}/"),
        format!("http://127.0.0.1:{lab_port}/about"),
        format!("http://127.0.0.1:{lab_port}/pricing"),
        format!("http://127.0.0.1:{lab_port}/login"),
        format!("http://127.0.0.1:{lab_port}/app/north"),
        format!("http://127.0.0.1:{lab_port}/app/north/reports"),
        format!("http://127.0.0.1:{lab_port}/app/north/items?page=1"),
        format!("http://127.0.0.1:{lab_port}/app/north/settings"),
        format!("http://127.0.0.1:{lab_port}/app/north/prompt-injection"),
        format!("http://127.0.0.1:{lab_port}/assets/app.js"),
        format!("http://127.0.0.1:{lab_port}/secret-note.txt"),
    ];

    let assessment_id = match db
        .assessment_by_name(workspace, company_id, "Synthetic lab assessment")
        .await?
    {
        Some(id) => id,
        None => {
            let targets = targets
                .iter()
                .filter_map(|raw| darkbox_domain::EndpointKey::parse(raw, "GET").ok())
                .collect();
            db.create_assessment(
                workspace,
                darkbox_storage::NewAssessment {
                    company_id,
                    name: "Synthetic lab assessment".into(),
                    environment: "lab".into(),
                    objective:
                        "Map the synthetic application, reproduce authenticated flows with two identities, and leave evidence another session can resume from."
                            .into(),
                    policy,
                    targets,
                    actor: "seed".into(),
                },
            )
            .await?
            .id
        }
    };

    if db
        .assessment_by_name(other_workspace, other_company, "Isolation assessment")
        .await?
        .is_none()
    {
        db.create_assessment(
            other_workspace,
            darkbox_storage::NewAssessment {
                company_id: other_company,
                name: "Isolation assessment".into(),
                environment: "lab".into(),
                objective: "Prove that identifiers never cross workspaces.".into(),
                policy: ScopePolicy::demo(lab_port),
                targets: Vec::new(),
                actor: "seed".into(),
            },
        )
        .await?;
    }

    db.create_tenant(workspace, assessment_id, "north", "North tenant")
        .await?;
    db.create_tenant(workspace, assessment_id, "south", "South tenant")
        .await?;

    if db
        .identity_by_label(assessment_id, "alice")
        .await?
        .is_none()
    {
        db.create_identity(
            workspace,
            assessment_id,
            &CreateIdentityRequest {
                label: "alice".into(),
                role_label: "admin".into(),
                tenant_keys: vec!["north".into(), "south".into()],
                capabilities: vec!["navigate".into(), "read".into()],
                secret_ref: Some("env:LAB_ALICE_PASSWORD".into()),
                login_config: serde_json::json!({
                    "login_url": format!("{lab_url}/login"),
                    "success_text": "Signed in as"
                }),
            },
        )
        .await?;
    }
    for (label, secret_env, tenants) in [
        ("alice", "LAB_ALICE_PASSWORD", vec!["north", "south"]),
        ("bob", "LAB_BOB_PASSWORD", vec!["north"]),
    ] {
        if let Some(identity_id) = db.identity_by_label(assessment_id, label).await? {
            let _ = db
                .update_identity(
                    workspace,
                    assessment_id,
                    identity_id,
                    &UpdateIdentityRequest {
                        role_label: None,
                        tenant_keys: Some(tenants.into_iter().map(str::to_string).collect()),
                        capabilities: None,
                        secret_ref: Some(Some(format!("env:{secret_env}"))),
                        login_config: Some(serde_json::json!({
                            "login_url": format!("{lab_url}/login"),
                            "success_text": "Signed in as",
                            "failure_text": "Invalid credentials"
                        })),
                    },
                )
                .await;
        }
    }
    if db.identity_by_label(assessment_id, "bob").await?.is_none() {
        db.create_identity(
            workspace,
            assessment_id,
            &CreateIdentityRequest {
                label: "bob".into(),
                role_label: "member".into(),
                tenant_keys: vec!["north".into()],
                capabilities: vec!["navigate".into(), "read".into()],
                secret_ref: Some("env:LAB_BOB_PASSWORD".into()),
                login_config: serde_json::json!({
                    "login_url": format!("{lab_url}/login"),
                    "success_text": "Signed in as"
                }),
            },
        )
        .await?;
    }

    let procedures_dir =
        PathBuf::from(std::env::var("DARKBOX_REPO_ROOT").unwrap_or_else(|_| ".".into()))
            .join("procedures");
    if procedures_dir.exists() {
        sync_procedures(db, &procedures_dir).await?;
    }

    if db
        .get_memory_page(workspace, assessment_id, "assessment-state")
        .await
        .is_err()
    {
        db.save_memory_page(
            workspace,
            assessment_id,
            "assessment-state",
            "Assessment state",
            "# Assessment state\n\nThe synthetic assessment is configured. No run has been executed yet.\n\nScope and identities are recorded; every future claim must point at an observation or be labelled as a hypothesis.\n",
            &[MemoryReferenceDto {
                kind: "assessment".into(),
                id: assessment_id.to_string(),
                label: "Synthetic lab assessment".into(),
            }],
            "current",
            "seed",
        )
        .await?;
    }

    println!("workspace       {workspace}");
    println!("other workspace {other_workspace}");
    println!("assessment      {assessment_id}");
    println!("token           {DEV_TOKEN}");
    println!("other token     {OTHER_TOKEN}");
    println!("identities      alice (admin, north+south), bob (member, north)");
    println!(
        "lab passwords   LAB_ALICE_PASSWORD=alice-lab-pass LAB_BOB_PASSWORD=bob-lab-pass (export before starting the worker)"
    );
    Ok(())
}

async fn sync_procedures(db: &Db, dir: &PathBuf) -> Result<usize> {
    let mut count = 0;
    let mut entries: Vec<_> = std::fs::read_dir(dir)?.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.path());
    for entry in entries {
        let path = entry.path();
        if path.extension().and_then(|v| v.to_str()) != Some("yaml") {
            continue;
        }
        let text = std::fs::read_to_string(&path)?;
        let manifest: serde_yaml::Value = serde_yaml::from_str(&text)?;
        let get = |key: &str| {
            manifest
                .get(key)
                .cloned()
                .unwrap_or(serde_yaml::Value::Null)
        };
        let id = get("id")
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("{}: missing id", path.display()))?
            .to_string();
        let version = get("version")
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("{}: missing version", path.display()))?
            .to_string();
        let name = get("name").as_str().unwrap_or(&id).to_string();
        let purpose = get("purpose").as_str().unwrap_or("").to_string();
        let target_types: Vec<String> =
            serde_yaml::from_value(get("target_types")).unwrap_or_else(|_| vec!["endpoint".into()]);
        let required: Vec<String> =
            serde_yaml::from_value(get("required_capabilities")).unwrap_or_default();
        let preconditions: Vec<String> =
            serde_yaml::from_value(get("preconditions")).unwrap_or_default();
        let effect: ActionClass = get("effect_class")
            .as_str()
            .unwrap_or("query")
            .parse()
            .map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        let manifest_json: serde_json::Value = serde_yaml::from_str(&text)?;
        db.upsert_procedure(NewProcedure {
            key: ProcedureKey::new(&id, &version),
            id,
            version,
            name,
            purpose,
            target_types,
            effect_class: effect,
            required_capabilities: required,
            preconditions,
            manifest: manifest_json,
        })
        .await?;
        count += 1;
    }
    Ok(count)
}

async fn purge(db: &Db) -> Result<usize> {
    let root = std::env::var("DARKBOX_BLOB_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("data/objects"));
    let store = darkbox_evidence::LocalBlobStore::new(root)?;
    let mut removed = 0;
    for (id, object_key) in db.expired_evidence(1000).await? {
        use darkbox_evidence::BlobStore;
        store.delete(&object_key).await.ok();
        db.delete_evidence(id).await?;
        removed += 1;
    }
    db.purge_all_memory_versions(5).await?;
    Ok(removed)
}
