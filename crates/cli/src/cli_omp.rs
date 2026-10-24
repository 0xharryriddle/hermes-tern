use clap::{Args, Subcommand};
use hermes_tern_core::error::Result;
use std::path::PathBuf;

#[derive(Args)]
pub(crate) struct HandoffArgs {
    #[command(subcommand)]
    command: HandoffCommand,
}
#[derive(Subcommand)]
enum HandoffCommand {
    /// One bounded JSON document on stdin; no agent launch or automatic send
    Exchange {
        /// Explicit private local state; separate from coordination state
        #[arg(long)]
        state: PathBuf,
    },
}
impl HandoffArgs {
    pub(crate) fn run(self) -> Result<()> {
        match self.command {
            HandoffCommand::Exchange { state } => {
                println!("{}", hermes_tern_omp::run_stdin(&state)?);
                Ok(())
            }
        }
    }
}
