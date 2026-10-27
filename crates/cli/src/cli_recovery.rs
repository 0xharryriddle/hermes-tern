//! Explicit offline archive diagnostics and retained-key publication; never open SQLite.
use clap::Args;
use hermes_tern_core::error::Result;
use std::path::PathBuf;

#[derive(Args)]
pub struct RecoveryArgs {
    #[arg(long)]
    state: PathBuf,
    #[arg(long)]
    workspace: PathBuf,
    #[arg(long)]
    archive_id: String,
    #[arg(long)]
    workspace_id: String,
    #[arg(long)]
    database_sha256: String,
    /// Separately retained exact key digest; never paste the key itself
    #[arg(long)]
    expected_operator_key_sha256: String,
}
impl RecoveryArgs {
    pub fn run(self) -> Result<()> {
        let report = hermes_tern_supervisor::recovery::preflight(
            &self.state,
            &self.workspace,
            &self.archive_id,
            &self.workspace_id,
            &self.database_sha256,
            &self.expected_operator_key_sha256,
        )?;
        println!("{report}");
        Ok(())
    }
}

#[derive(Args)]
pub struct OperatorKeyArgs {
    #[arg(long)]
    state: PathBuf,
    #[arg(long)]
    staging_id: String,
    /// Separately retained digest; never put the secret itself in arguments
    #[arg(long)]
    expected_operator_key_sha256: String,
    /// Operator independently attests these are the intact original key bytes
    #[arg(long)]
    attest_original_key: bool,
}
impl OperatorKeyArgs {
    pub fn run(self) -> Result<()> {
        let report = hermes_tern_supervisor::operator_key_recovery::recover(
            &self.state,
            &self.staging_id,
            &self.expected_operator_key_sha256,
            self.attest_original_key,
        )?;
        println!("{report}");
        Ok(())
    }
}
