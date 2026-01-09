mod config;
mod image;
mod jail;
mod runtime;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;

#[derive(Parser)]
#[command(name = "jail")]
#[command(about = "Sandboxed dev environments via containers", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Clone a git repository or local path into a sandboxed environment
    Clone {
        /// Git URL or local path to clone
        source: String,
        /// Name for the jail (default: derived from source)
        #[arg(short, long)]
        name: Option<String>,
    },
    /// List all jails
    List,
    /// Enter a jail's shell
    Enter {
        /// Name of the jail
        name: String,
    },
    /// Remove a jail
    Remove {
        /// Name of the jail
        name: String,
    },
    /// Open VSCode attached to a jail's container
    Code {
        /// Name of the jail
        name: String,
    },
    /// Check runtime health status
    Status,
}

fn main() {
    if let Err(e) = run() {
        eprintln!("{} {}", "error:".red().bold(), e);
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Clone { source, name } => jail::clone(&source, name.as_deref())?,
        Commands::List => jail::list()?,
        Commands::Enter { name } => jail::enter(&name)?,
        Commands::Remove { name } => jail::remove(&name)?,
        Commands::Code { name } => jail::code(&name)?,
        Commands::Status => jail::status()?,
    }

    Ok(())
}
