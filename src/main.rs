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
        /// Ports to expose (can be specified multiple times)
        #[arg(short, long = "port", action = clap::ArgAction::Append)]
        ports: Vec<u16>,
    },
    /// Create an empty jail
    Create {
        /// Name for the jail
        name: String,
        /// Ports to expose (can be specified multiple times)
        #[arg(short, long = "port", action = clap::ArgAction::Append)]
        ports: Vec<u16>,
    },
    /// List all jails
    List,
    /// Enter a jail's shell
    Enter {
        /// Name or filter for the jail (interactive selection if multiple match)
        name: Option<String>,
        /// Ports to expose (can be specified multiple times, will recreate container if needed)
        #[arg(short, long = "port", action = clap::ArgAction::Append)]
        ports: Vec<u16>,
    },
    /// Alias for enter
    #[command(hide = true)]
    Start {
        name: Option<String>,
        #[arg(short, long = "port", action = clap::ArgAction::Append)]
        ports: Vec<u16>,
    },
    /// Remove a jail
    Remove {
        /// Name or filter for the jail (interactive selection if multiple match)
        name: Option<String>,
    },
    /// Alias for remove
    #[command(hide = true)]
    Rm { name: Option<String> },
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
        Commands::Clone {
            source,
            name,
            ports,
        } => jail::clone(&source, name.as_deref(), ports)?,
        Commands::Create { name, ports } => jail::create(&name, ports)?,
        Commands::List => jail::list()?,
        Commands::Enter { name, ports } | Commands::Start { name, ports } => {
            jail::enter(name.as_deref(), ports)?
        }
        Commands::Remove { name } | Commands::Rm { name } => jail::remove(name.as_deref())?,
        Commands::Code { name } => jail::code(&name)?,
        Commands::Status => jail::status()?,
    }

    Ok(())
}
