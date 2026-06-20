use clap::{Args, Parser, Subcommand};
use ngit_core::{
    accept, add_annotation, add_evidence_from_file, capture, doctor, export_json_schemas, history,
    init, lineage, list_annotations, list_drafts, migrate_legacy, reject,
    run_evidence_with_options, show_record, status, watch_capture_once, watch_once,
    AnnotationInput, CaptureOptions, CoreResult, DraftChange, EventRecord, EvidenceRunOptions,
};
use serde_json::json;
use std::{path::PathBuf, process::ExitCode, thread, time::Duration};

#[derive(Debug, Parser)]
#[command(name = "ngit")]
#[command(about = "Local code-editing blackbox for evidence, intent, rationale, and lineage")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Init(JsonFlag),
    Status(JsonFlag),
    Watch(WatchArgs),
    Capture(CaptureArgs),
    Drafts(JsonFlag),
    Show(ShowArgs),
    Annotation {
        #[command(subcommand)]
        command: AnnotationCommand,
    },
    Evidence {
        #[command(subcommand)]
        command: EvidenceCommand,
    },
    Accept(AcceptArgs),
    Reject(RejectArgs),
    History(JsonFlag),
    Lineage(ShowArgs),
    Doctor(JsonFlag),
    Schema {
        #[command(subcommand)]
        command: SchemaCommand,
    },
    Migrate(JsonFlag),
    Tui(TuiArgs),
    Serve(ServeArgs),
}

#[derive(Debug, Args)]
struct JsonFlag {
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct WatchArgs {
    #[arg(long)]
    once: bool,
    #[arg(long, default_value_t = 1)]
    interval: u64,
    #[arg(long)]
    json: bool,
    #[arg(long)]
    capture: bool,
}

#[derive(Debug, Args)]
struct TuiArgs {
    #[arg(long)]
    read_only: bool,
}

#[derive(Debug, Args)]
struct ServeArgs {
    #[arg(long, default_value = "127.0.0.1:7878")]
    bind: String,
    #[arg(long)]
    once: bool,
    #[arg(long)]
    token: Option<String>,
    #[arg(long)]
    allow_non_loopback: bool,
    #[arg(long)]
    require_auth_for_read: bool,
}

#[derive(Debug, Subcommand)]
enum SchemaCommand {
    Export(SchemaExportArgs),
}

#[derive(Debug, Args)]
struct SchemaExportArgs {
    #[arg(long)]
    dir: PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct CaptureArgs {
    #[arg(long, default_value = "manual")]
    trigger: String,
    #[arg(long)]
    intent: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct ShowArgs {
    id: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum AnnotationCommand {
    Add(AnnotationAddArgs),
    List(AnnotationListArgs),
    Show(ShowArgs),
}

#[derive(Debug, Args)]
struct AnnotationAddArgs {
    owner_id: String,
    #[arg(long = "type")]
    annotation_type: String,
    #[arg(long)]
    body: String,
    #[arg(long)]
    summary: Option<String>,
    #[arg(long, default_value = "human")]
    producer: String,
    #[arg(long, default_value = "supplied")]
    status: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct AnnotationListArgs {
    owner_id: String,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Subcommand)]
enum EvidenceCommand {
    Add(EvidenceAddArgs),
    Run(EvidenceRunArgs),
}

#[derive(Debug, Args)]
struct EvidenceAddArgs {
    draft_id: String,
    #[arg(long)]
    file: PathBuf,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct EvidenceRunArgs {
    draft_id: String,
    #[arg(long, default_value_t = 120)]
    timeout_seconds: u64,
    #[arg(long, default_value_t = 5 * 1024 * 1024)]
    max_output_bytes: usize,
    #[arg(long)]
    json: bool,
    #[arg(last = true, required = true)]
    command: Vec<String>,
}

#[derive(Debug, Args)]
struct AcceptArgs {
    draft_id: String,
    #[arg(long)]
    override_reason: Option<String>,
    #[arg(long)]
    json: bool,
}

#[derive(Debug, Args)]
struct RejectArgs {
    draft_id: String,
    #[arg(long)]
    reason: String,
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("ngit: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> CoreResult<()> {
    let cli = Cli::parse();
    let cwd = std::env::current_dir().map_err(|source| ngit_core::CoreError::Io {
        path: PathBuf::from("."),
        source,
    })?;
    match cli.command {
        Commands::Init(args) => {
            let record = init(&cwd)?;
            output(args.json, &record, || {
                format!(
                    "initialized .ngit at {}",
                    record.repo.root.join(".ngit").display()
                )
            })
        }
        Commands::Status(args) => {
            let record = status(&cwd)?;
            output(args.json, &record, || {
                format!(
                    "repo: {}\nbranch: {}\ndirty: {}\nopen drafts: {}",
                    record.repo_root.display(),
                    record.snapshot.branch,
                    record.snapshot.dirty,
                    record.open_drafts
                )
            })
        }
        Commands::Watch(args) => loop {
            if args.capture {
                let (event, draft) = watch_capture_once(&cwd)?;
                print_watch_capture(args.json, &event, draft.as_ref())?;
            } else {
                let event = watch_once(&cwd)?;
                print_watch_event(args.json, &event)?;
            }
            if args.once {
                break Ok(());
            }
            thread::sleep(Duration::from_secs(args.interval.max(1)));
        },
        Commands::Capture(args) => {
            let record = capture(
                &cwd,
                CaptureOptions {
                    trigger: args.trigger,
                    intent: args.intent,
                },
            )?;
            output(args.json, &record, || {
                format!("captured {}: {}", record.draft_id, record.summary)
            })
        }
        Commands::Drafts(args) => {
            let records = list_drafts(&cwd)?;
            output(args.json, &records, || {
                records
                    .iter()
                    .map(|draft| {
                        format!(
                            "{} evidence_state={}",
                            draft.draft_id, draft.readiness.final_action
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
        }
        Commands::Show(args) => {
            let record = show_record(&cwd, &args.id)?;
            output(args.json, &record, || format!("{record:#?}"))
        }
        Commands::Annotation { command } => match command {
            AnnotationCommand::Add(args) => {
                let record = add_annotation(
                    &cwd,
                    AnnotationInput {
                        owner_id: args.owner_id,
                        annotation_type: args.annotation_type,
                        status: args.status,
                        summary: args.summary,
                        body: args.body,
                        producer_kind: args.producer,
                    },
                )?;
                output(args.json, &record, || {
                    format!("added annotation {}", record.annotation_id)
                })
            }
            AnnotationCommand::List(args) => {
                let records = list_annotations(&cwd, &args.owner_id)?;
                output(args.json, &records, || {
                    records
                        .iter()
                        .map(|record| {
                            format!(
                                "{} {} {}",
                                record.annotation_id,
                                record.annotation_type,
                                record.summary.clone().unwrap_or_default()
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                })
            }
            AnnotationCommand::Show(args) => {
                let record = show_record(&cwd, &args.id)?;
                output(args.json, &record, || format!("{record:#?}"))
            }
        },
        Commands::Evidence { command } => match command {
            EvidenceCommand::Add(args) => {
                let record = add_evidence_from_file(&cwd, &args.draft_id, &args.file)?;
                output(args.json, &record, || {
                    format!("added evidence {}", record.evidence_id)
                })
            }
            EvidenceCommand::Run(args) => {
                let record = run_evidence_with_options(
                    &cwd,
                    &args.draft_id,
                    &args.command,
                    EvidenceRunOptions {
                        timeout: Duration::from_secs(args.timeout_seconds),
                        max_output_bytes: args.max_output_bytes,
                    },
                )?;
                output(args.json, &record, || {
                    format!("evidence {} {}", record.evidence_id, record.status)
                })
            }
        },
        Commands::Accept(args) => {
            let record = accept(&cwd, &args.draft_id, args.override_reason)?;
            output(args.json, &record, || {
                format!("accepted {}", record.change_id)
            })
        }
        Commands::Reject(args) => {
            let record = reject(&cwd, &args.draft_id, args.reason)?;
            output(args.json, &record, || {
                format!("rejected {}", record.draft_id)
            })
        }
        Commands::History(args) => {
            let records = history(&cwd)?;
            output(args.json, &records, || {
                format!("{} terminal records", records.len())
            })
        }
        Commands::Lineage(args) => {
            let record = lineage(&cwd, &args.id)?;
            output(args.json, &record, || format!("{record:#?}"))
        }
        Commands::Doctor(args) => {
            let record = doctor(&cwd)?;
            output(args.json, &record, || {
                if record.issues.is_empty() {
                    "doctor: ok".to_string()
                } else {
                    format!("doctor: {} issue(s)", record.issues.len())
                }
            })
        }
        Commands::Schema { command } => match command {
            SchemaCommand::Export(args) => {
                let record = export_json_schemas(&args.dir)?;
                output(args.json, &record, || {
                    format!("exported {} schema files", record.files.len())
                })
            }
        },
        Commands::Migrate(args) => {
            let record = migrate_legacy(&cwd)?;
            output(args.json, &record, || {
                format!(
                    "migration imported {} records; skipped {}",
                    record.imported.values().sum::<usize>(),
                    record.skipped.len()
                )
            })
        }
        Commands::Tui(args) => {
            let record = status(&cwd)?;
            let drafts = list_drafts(&cwd)?;
            println!(
                "{}",
                ngit_tui::render_overview(&record, &drafts, args.read_only)?
            );
            Ok(())
        }
        Commands::Serve(args) => ngit_serve::serve_with_options(
            &cwd,
            ngit_serve::ServeOptions {
                bind: args.bind,
                once: args.once,
                token: args.token,
                allow_non_loopback: args.allow_non_loopback,
                require_auth_for_read: args.require_auth_for_read,
            },
        )
        .map_err(|err| ngit_core::CoreError::InvalidInput(err.to_string())),
    }
}

fn output<T, F>(json: bool, value: &T, human: F) -> CoreResult<()>
where
    T: serde::Serialize,
    F: FnOnce() -> String,
{
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(value).map_err(|source| {
                ngit_core::CoreError::MalformedJson {
                    path: PathBuf::from("<stdout>"),
                    source,
                }
            })?
        );
    } else {
        println!("{}", human());
    }
    Ok(())
}

fn print_watch_event(json_output: bool, event: &EventRecord) -> CoreResult<()> {
    if json_output {
        return output_compact(event);
    }
    output(false, event, || {
        format!(
            "{} {} dirty_files={}",
            event.event_id,
            event.event_type,
            event.changed_files.len()
        )
    })
}

fn print_watch_capture(
    json_output: bool,
    event: &EventRecord,
    draft: Option<&DraftChange>,
) -> CoreResult<()> {
    let payload = json!({ "event": event, "draft": draft });
    if json_output {
        return output_compact(&payload);
    }
    output(false, &payload, || {
        if let Some(draft) = draft {
            format!("{} captured {}", event.event_id, draft.draft_id)
        } else {
            format!("{} {} no capture", event.event_id, event.event_type)
        }
    })
}

fn output_compact<T: serde::Serialize>(value: &T) -> CoreResult<()> {
    println!(
        "{}",
        serde_json::to_string(value).map_err(|source| ngit_core::CoreError::MalformedJson {
            path: PathBuf::from("<stdout>"),
            source,
        })?
    );
    Ok(())
}
