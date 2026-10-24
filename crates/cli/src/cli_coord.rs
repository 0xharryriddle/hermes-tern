//! Human operator CLI and explicitly credential-scoped peer requests.
use clap::{Args, Subcommand};
use hermes_tern_core::error::Result;
use hermes_tern_store::{DispositionInput, MessageInput, Store, StoreLimits};
use hermes_tern_supervisor::{Client, Operation, Supervisor};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Args)]
pub struct MigrateArgs {
    #[arg(long)]
    state: PathBuf,
    #[arg(long)]
    workspace: PathBuf,
    #[arg(long)]
    archive_id: String,
}
impl MigrateArgs {
    pub fn run(self) -> Result<()> {
        let receipt = Store::migrate(
            &self.state,
            &self.workspace,
            &self.archive_id,
            StoreLimits::default(),
        )?;
        println!("{receipt}");
        Ok(())
    }
}
#[derive(Args)]
pub struct ServeArgs {
    /// Private state directory (new or mode 0700); never Hermes' own home
    #[arg(long)]
    pub state: PathBuf,
    /// Canonical approved workspace; not a sandbox
    #[arg(long)]
    pub workspace: PathBuf,
}
impl ServeArgs {
    pub fn run(self) -> Result<()> {
        let server = Supervisor::bind(&self.state, &self.workspace, StoreLimits::default())?;
        eprintln!("hermes-tern supervisor ready; foreground, Linux local IPC, no agents launched");
        server.run(Arc::new(AtomicBool::new(false)))
    }
}
#[derive(Args)]
pub struct CoordArgs {
    #[arg(long)]
    pub state: PathBuf,
    /// Explicit actor credential file; omit only for operator commands
    #[arg(long)]
    pub credential_file: Option<PathBuf>,
    #[command(subcommand)]
    pub operation: CoordCommand,
}
#[derive(Subcommand)]
pub enum CoordCommand {
    Status,
    /// Provision a peer binding; caller must verify this physical session and profile
    BindPeer {
        #[arg(long)]
        actor: String,
        #[arg(long)]
        hermes_home: PathBuf,
        #[arg(long)]
        physical_session: String,
        #[arg(long)]
        attest_binding: bool,
    },
    Actors,
    Messages,
    /// Read bounded metadata and an observation cursor; no delivery or mutation
    Snapshot,
    /// Start a revision-bound, read-only enumeration of all retained coordination state
    StateIndex,
    /// Read one bounded state page; discard partial assembly when restart_required
    StatePage {
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        epoch: String,
        #[arg(long)]
        revision: String,
        #[arg(long, value_parser=["actors","routes","messages","tasks","dispositions","migrations","events"])]
        section: String,
        #[arg(long, default_value = "0")]
        offset: String,
        #[arg(long, default_value_t=64, value_parser=clap::value_parser!(u16).range(1..=128))]
        limit: u16,
    },
    /// Capture a private SQLite snapshot; never prune source rows or restore authority
    Archive {
        #[arg(long)]
        archive_id: String,
    },
    /// Verify a previously published archive without activating it
    InspectArchive {
        #[arg(long)]
        archive_id: String,
    },
    /// Read-only verification of an exact retained staging UUID; never guess a path
    InspectStagedArchive {
        #[arg(long)]
        staging_id: String,
        #[arg(long)]
        archive_id: String,
    },
    /// Finish publication of an exact verified bundle; no restore or source pruning
    RecoverArchive {
        #[arg(long)]
        staging_id: String,
        #[arg(long)]
        archive_id: String,
        #[arg(long)]
        database_sha256: String,
    },
    /// Read event notices; changed epoch or replay gaps require a new snapshot
    Events {
        #[arg(long)]
        workspace: String,
        #[arg(long)]
        epoch: String,
        #[arg(long)]
        after: String,
        #[arg(long, default_value_t=64, value_parser=clap::value_parser!(u16).range(1..=128))]
        limit: u16,
    },
    /// Inspect one exact sender/message key without delivering it (operator only)
    InspectMessage {
        #[arg(long)]
        sender: String,
        #[arg(long)]
        message: String,
    },
    /// Record an immutable decision on an ambiguous delivery; never assert consumption
    DisposeMessage {
        #[arg(long)]
        sender: String,
        #[arg(long)]
        message: String,
        #[arg(long)]
        delivery_incarnation: String,
        #[arg(long)]
        decision_id: String,
        #[arg(long, value_parser=["close","reoffer"])]
        action: String,
        #[arg(long)]
        successor_id: Option<String>,
        #[arg(long, value_parser=clap::value_parser!(u64).range(1..=3600))]
        ttl: Option<u64>,
        #[arg(long)]
        reason: String,
        #[arg(long)]
        attest_outcome_unknown: bool,
    },
    /// Inspect one task and its attributed result without changing ownership
    InspectTask {
        #[arg(long)]
        task: String,
    },
    /// Register an operator-attested runtime binding; does not launch or prove liveness
    Register {
        #[arg(long,value_parser=["hermes","omp"])]
        kind: String,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        session: String,
        #[arg(long)]
        model: String,
        #[arg(long)]
        provider: String,
    },
    Rotate {
        #[arg(long)]
        actor: String,
    },
    /// Verify a retained credential against current authority without publishing it
    InspectStagedCredential {
        #[arg(long)]
        staging_id: String,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        incarnation: String,
    },
    /// Finish publication of an exact retained current credential; never rotate authority
    RecoverCredential {
        #[arg(long)]
        staging_id: String,
        #[arg(long)]
        actor: String,
        #[arg(long)]
        incarnation: String,
    },
    /// Routes default to denied; --allow enables this exact direction
    Route {
        #[arg(long)]
        sender: String,
        #[arg(long)]
        recipient: String,
        #[arg(long)]
        allow: bool,
    },
    Send {
        #[arg(long)]
        recipient: String,
        /// Stable UUID chosen before sending; reuse only with identical content
        #[arg(long)]
        message_id: String,
        #[arg(long,default_value="context",value_parser=["context","task_offer","task_result","question","answer"])]
        kind: String,
        #[arg(long)]
        text: String,
        #[arg(long,default_value_t=300,value_parser=clap::value_parser!(u64).range(1..=3600))]
        ttl: u64,
        #[arg(long)]
        correlation_id: Option<String>,
    },
    Receive,
    Ack {
        #[arg(long)]
        sender: String,
        #[arg(long)]
        message: String,
    },
    Claim {
        #[arg(long)]
        task: String,
        #[arg(long, default_value_t = 300)]
        ttl: u64,
    },
    Complete {
        #[arg(long)]
        task: String,
        #[arg(long)]
        generation: String,
        #[arg(long)]
        result: String,
    },
    /// Release the exact inspected lease with stopped attestation; does not stop its writer
    ReleaseStopped {
        #[arg(long)]
        task: String,
        #[arg(long)]
        owner: String,
        #[arg(long)]
        incarnation: String,
        #[arg(long)]
        generation: String,
        #[arg(long)]
        attest_stopped: bool,
    },
    /// Stop the coordinator only; never signal Hermes/OMP
    Shutdown,
}
impl CoordArgs {
    pub fn run(self) -> Result<()> {
        let client = match self.credential_file {
            Some(path) => Client::peer(&self.state, &path)?,
            None => Client::operator(&self.state)?,
        };
        let op = match self.operation {
            CoordCommand::BindPeer {
                actor,
                hermes_home,
                physical_session,
                attest_binding,
            } => Operation::BindPeer {
                actor,
                hermes_home,
                physical_session_id: physical_session,
                attest_binding,
            },
            CoordCommand::Status => Operation::Hello {},
            CoordCommand::Actors => Operation::Actors {},
            CoordCommand::Messages => Operation::Messages {},
            CoordCommand::Snapshot => Operation::Snapshot {},
            CoordCommand::StateIndex => Operation::StateIndex {},
            CoordCommand::StatePage {
                workspace,
                epoch,
                revision,
                section,
                offset,
                limit,
            } => Operation::StatePage {
                workspace,
                epoch,
                revision,
                section,
                offset,
                limit,
            },
            CoordCommand::Archive { archive_id } => Operation::Archive { archive_id },
            CoordCommand::InspectArchive { archive_id } => Operation::InspectArchive { archive_id },
            CoordCommand::InspectStagedArchive {
                staging_id,
                archive_id,
            } => Operation::InspectStagedArchive {
                staging_id,
                archive_id,
            },
            CoordCommand::RecoverArchive {
                staging_id,
                archive_id,
                database_sha256,
            } => Operation::RecoverArchive {
                staging_id,
                archive_id,
                database_sha256,
            },
            CoordCommand::Events {
                workspace,
                epoch,
                after,
                limit,
            } => Operation::Events {
                workspace,
                epoch,
                after,
                limit,
            },
            CoordCommand::InspectMessage { sender, message } => {
                Operation::InspectMessage { sender, message }
            }
            CoordCommand::DisposeMessage {
                sender,
                message,
                delivery_incarnation,
                decision_id,
                action,
                successor_id,
                ttl,
                reason,
                attest_outcome_unknown,
            } => Operation::DisposeMessage {
                disposition: DispositionInput {
                    sender,
                    message,
                    delivery_incarnation,
                    decision_id,
                    action,
                    successor_id,
                    ttl_seconds: ttl,
                    reason,
                    attest_outcome_unknown,
                },
            },
            CoordCommand::InspectTask { task } => Operation::InspectTask { task },
            CoordCommand::Register {
                kind,
                profile,
                session,
                model,
                provider,
            } => Operation::Register {
                kind,
                profile,
                session,
                model,
                provider,
            },
            CoordCommand::Rotate { actor } => Operation::Rotate { actor },
            CoordCommand::InspectStagedCredential {
                staging_id,
                actor,
                incarnation,
            } => Operation::InspectStagedCredential {
                staging_id,
                actor,
                incarnation,
            },
            CoordCommand::RecoverCredential {
                staging_id,
                actor,
                incarnation,
            } => Operation::RecoverCredential {
                staging_id,
                actor,
                incarnation,
            },
            CoordCommand::Route {
                sender,
                recipient,
                allow,
            } => Operation::Route {
                sender,
                recipient,
                allow,
            },
            CoordCommand::Send {
                recipient,
                message_id,
                kind,
                text,
                ttl,
                correlation_id,
            } => Operation::Send {
                message: MessageInput {
                    message_id,
                    recipient,
                    kind,
                    payload: json!({"text":text}),
                    ttl_seconds: ttl,
                    correlation_id,
                },
            },
            CoordCommand::Receive => Operation::Receive {},
            CoordCommand::Ack { sender, message } => Operation::Ack { sender, message },
            CoordCommand::Claim { task, ttl } => Operation::Claim {
                task,
                ttl_seconds: ttl,
            },
            CoordCommand::Complete {
                task,
                generation,
                result,
            } => Operation::Complete {
                task,
                generation,
                result,
            },
            CoordCommand::ReleaseStopped {
                task,
                owner,
                incarnation,
                generation,
                attest_stopped,
            } => Operation::ReleaseStopped {
                task,
                owner,
                incarnation,
                generation,
                attest_stopped,
            },
            CoordCommand::Shutdown => Operation::Shutdown {},
        };
        println!("{}", client.call(op)?);
        Ok(())
    }
}
