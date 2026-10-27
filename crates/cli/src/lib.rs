use std::{path::PathBuf, process::ExitCode, time::Duration};

use crate::cli_coord::{CoordArgs, MigrateArgs, ServeArgs};
use clap::{Args, CommandFactory, Parser, Subcommand};
use hermes_tern_core::error::Error;
use hermes_tern_gateway::{
    config::{AttachOptions, Limits, SessionOptions},
    inspect,
};
use hermes_tern_ui::{app, preview};

mod cli_coord;
mod cli_omp;
mod cli_recovery;

#[derive(Parser)]
#[command(name = "hermes-tern", version, about = "Hermes native client for Tern")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}
#[derive(Subcommand)]
enum Commands {
    /// Explicit operator OMP proposals and dispatch fence; does not send prompts
    OmpHandoff(cli_omp::HandoffArgs),
    /// Foreground durable mailbox supervisor; launches no agents
    Serve(ServeArgs),
    /// Operator/peer coordination with distinct file-bound credentials
    Coord(CoordArgs),
    /// Offline v1->v2 upgrade with verified archival; refuses a live coordinator
    MigrateStore(MigrateArgs),
    /// Offline exact archive/key diagnostic; never restores or activates authority
    PreflightRecovery(cli_recovery::RecoveryArgs),
    /// Offline publish one operator-attested retained original key; never open SQLite
    RecoverOperatorKey(cli_recovery::OperatorKeyArgs),
    /// One bounded peer tool request on stdin; binding provisioned by an operator
    PeerTool {
        #[arg(long)]
        binding: PathBuf,
    },
    /// Static SDK preview; never launch an agent
    Preview,
    /// Discover an existing gateway and verify capabilities; no inference
    Doctor(RuntimeArgs),
    /// Native session picker; New/Resume require explicit human activation
    Open {
        #[command(flatten)]
        runtime: RuntimeArgs,
        #[arg(long)]
        cwd: Option<PathBuf>,
    },
    /// Inspect one bounded history projection snapshot; never resume or send
    History {
        #[command(flatten)]
        runtime: RuntimeArgs,
        #[arg(long)]
        session: String,
        #[arg(long)]
        plain: bool,
        #[arg(long, default_value_t=1, value_parser=clap::value_parser!(u32).range(1..))]
        page: u32,
    },
    /// List live sessions, or saved sessions with --stored
    Sessions {
        #[command(flatten)]
        runtime: RuntimeArgs,
        #[arg(long)]
        stored: bool,
    },
    /// Native conversation; --plain selects text; --query runs one bounded turn
    Chat {
        #[command(flatten)]
        runtime: RuntimeArgs,
        #[arg(long)]
        cwd: Option<PathBuf>,
        #[arg(long)]
        resume: Option<String>,
        #[arg(long, conflicts_with = "resume")]
        model: Option<String>,
        #[arg(long, conflicts_with = "resume")]
        provider: Option<String>,
        #[arg(long)]
        plain: bool,
        #[arg(long)]
        query: Option<String>,
        #[arg(long, default_value_t=300, value_parser=clap::value_parser!(u64).range(1..=3600))]
        timeout: u64,
    },
}
#[derive(Args)]
struct RuntimeArgs {
    #[arg(long, env = "HERMES_TERN_ROOT")]
    hermes_root: Option<PathBuf>,
    #[arg(long, env = "HERMES_TERN_PYTHON")]
    python: Option<PathBuf>,
    #[arg(long)]
    home: Option<PathBuf>,
}
impl RuntimeArgs {
    fn options(self) -> Result<AttachOptions, Error> {
        let root = match self.hermes_root {
            Some(root) => root,
            None => PathBuf::from(
                std::env::var_os("HOME")
                    .ok_or(Error::Config("HOME unavailable; set --hermes-root"))?,
            )
            .join(".hermes/hermes-agent"),
        };
        let python = self.python.unwrap_or_else(|| root.join("venv/bin/python"));
        AttachOptions::new(root, python, self.home)
    }
}

/// Parse process arguments and dispatch to the owning library subsystem.
pub fn run() -> ExitCode {
    let cli = Cli::parse();
    let result = match cli.command {
        Some(Commands::OmpHandoff(args)) => args.run(),
        Some(Commands::Serve(args)) => args.run(),
        Some(Commands::Coord(args)) => args.run(),
        Some(Commands::MigrateStore(args)) => args.run(),
        Some(Commands::PreflightRecovery(args)) => args.run(),
        Some(Commands::RecoverOperatorKey(args)) => args.run(),
        Some(Commands::PeerTool { binding }) => {
            hermes_tern_supervisor::peer::run_stdin(&binding).map(|value| println!("{value}"))
        }
        None => Cli::command().print_help().map_err(Error::from),
        Some(Commands::Preview) => {
            preview::render().map_err(|_| Error::Transport("native preview failed"))
        }
        Some(Commands::Doctor(args)) => args.options().and_then(|options| {
            let report = inspect::doctor(&options, &Limits::default())?;
            println!("{report}");
            Ok(())
        }),
        Some(Commands::Open { runtime, cwd }) => runtime.options().and_then(|options| {
            hermes_tern_ui::launcher::open(options, cwd.map_or_else(std::env::current_dir, Ok)?)
        }),
        Some(Commands::Sessions { runtime, stored }) => runtime.options().and_then(|options| {
            println!("{}", app::sessions(&options, stored)?);
            Ok(())
        }),
        Some(Commands::History {
            runtime,
            session,
            plain,
            page,
        }) => runtime.options().and_then(|options| {
            hermes_tern_ui::history::show(&options, &session, plain, page as usize)
        }),
        Some(Commands::Chat {
            runtime,
            cwd,
            resume,
            model,
            provider,
            plain,
            query,
            timeout,
        }) => runtime.options().and_then(|attach| {
            let cwd = match cwd {
                Some(cwd) => cwd,
                None => std::env::current_dir()?,
            };
            app::chat(
                attach,
                SessionOptions {
                    cwd,
                    resume,
                    model,
                    provider,
                },
                plain,
                query,
                Duration::from_secs(timeout),
            )
        }),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("hermes-tern: {error}");
            ExitCode::FAILURE
        }
    }
}
