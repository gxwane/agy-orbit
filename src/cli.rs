use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "agyo",
    author,
    version,
    about = "Seamless Multi-Account Manager & Isolated Orbit Runner for Antigravity CLI",
    long_about = "A lightweight, secure, cross-platform tool to manage multiple Google Antigravity accounts without touching your session history or plugins."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Save current Antigravity credentials as a named Orbit
    #[command(alias = "s")]
    Save {
        /// Orbit identifier name (e.g., work, personal)
        name: String,

        /// Optional descriptive label for this account
        #[arg(short, long)]
        label: Option<String>,

        /// Overwrite if orbit name already exists
        #[arg(short, long)]
        force: bool,
    },

    /// Switch globally to the specified Orbit
    #[command(aliases = ["u", "sw"])]
    Use {
        /// Orbit identifier name to switch to
        name: String,
    },

    /// List all saved Orbits
    #[command(alias = "ls")]
    List,

    /// Show current active Orbit and account email
    #[command(alias = "w")]
    Whoami,

    /// Remove a saved Orbit
    #[command(alias = "rm")]
    Remove {
        /// Orbit identifier name to remove
        name: String,
    },

    /// Run a command in an isolated Orbit session with lifetime lease protection
    #[command(alias = "r")]
    Run {
        /// Orbit identifier name to activate
        name: String,

        /// Automatically restore to the previous active orbit upon session exit
        #[arg(long, default_value_t = false)]
        restore: bool,

        /// Command and arguments to execute (defaults to 'agy')
        #[arg(last = true)]
        cmd: Vec<String>,
    },

    /// Check quota and usage for active or specified Orbit
    #[command(alias = "q")]
    Quota {
        /// Optional Orbit identifier name (defaults to active orbit)
        name: Option<String>,
    },
}
