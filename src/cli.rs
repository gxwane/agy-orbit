use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "agyo",
    bin_name = "agyo",
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
    /// Save active credentials as a named Orbit
    #[command(
        visible_alias = "s",
        long_about = "Capture current active Antigravity credentials, account metadata, and system keyring secret as a named Orbit snapshot."
    )]
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
    #[command(
        alias = "switch",
        visible_aliases = ["u", "sw"],
        long_about = "Atomically update global Antigravity credentials and system keyring target to the specified Orbit using crash-resilient WAL state machine."
    )]
    Use {
        /// Orbit identifier name to switch to
        name: String,
    },

    /// List all saved Orbits
    #[command(
        visible_alias = "ls",
        long_about = "Display all saved Orbits with creation timestamp, account email, optional label, and active status indicator."
    )]
    List,

    /// Show current active Orbit and account email
    #[command(
        visible_alias = "w",
        long_about = "Inspect active Antigravity authentication files and system keyring to verify active identity against saved Orbits."
    )]
    Whoami,

    /// Remove a saved Orbit
    #[command(
        visible_alias = "rm",
        long_about = "Permanently remove a saved Orbit snapshot and purge its credentials from disk storage."
    )]
    Remove {
        /// Orbit identifier name to remove
        name: String,
    },

    /// Run a command in an isolated Orbit session
    #[command(
        visible_alias = "r",
        long_about = "Temporarily activates the specified Orbit credentials in the environment and executes the given command (defaults to 'agy'). Protects against keyring stepping with an OS-level lifetime lease lock, and performs two-way token synchronization upon process exit."
    )]
    Run {
        /// Orbit identifier name to activate
        name: String,

        /// Automatically restore to the previous active orbit upon session exit
        #[arg(long, default_value_t = false)]
        restore: bool,

        /// Command and arguments to execute (defaults to 'agy')
        #[arg(trailing_var_arg = true)]
        cmd: Vec<String>,
    },

    /// Check model quota and consumption status
    #[command(
        visible_alias = "q",
        long_about = "Check and monitor Google Cloud Code PA model quota and consumption status.\n\n\
                      By default, queries the currently active Orbit account.\n\
                      Pass [NAME] to inspect a specific saved Orbit without switching.\n\
                      Pass `-a, --all` to display an aggregated multi-account dashboard across all saved Orbits.\n\
                      Pass `-r, --refresh` to bypass local 60s cache and fetch fresh remote data."
    )]
    Quota {
        /// Target Orbit name (defaults to active orbit; mutually exclusive with --all)
        #[arg(conflicts_with = "all")]
        name: Option<String>,

        /// Force refresh from remote API, bypassing the 60s local cache
        #[arg(short, long)]
        refresh: bool,

        /// Query and display aggregated quota dashboard for all saved Orbits
        #[arg(short, long, conflicts_with = "name")]
        all: bool,
    },

    /// Generate shell completion scripts
    #[command(
        visible_alias = "comp",
        long_about = "Generate dynamic & static shell completion scripts for Bash, Zsh, Fish, PowerShell, or Elvish.\n\
                      Automatically detects your current shell environment when omitted in an interactive terminal."
    )]
    Completion {
        /// Target shell family (auto-detected if omitted)
        #[arg(value_enum)]
        shell: Option<clap_complete::Shell>,

        /// Output raw script without setup guide even in interactive terminal
        #[arg(long)]
        raw: bool,
    },

    /// Check for updates or self-upgrade agyo to the latest release
    #[command(
        visible_aliases = ["update", "up"],
        long_about = "Check for available new versions from GitHub Releases and perform an in-place atomic upgrade.\n\
                      Strictly verifies SHA-256 integrity and handles OS-level file locking with automatic rollback."
    )]
    Upgrade {
        /// Only check for updates without downloading or installing
        #[arg(short, long)]
        check: bool,

        /// Force reinstall or upgrade even if already on the latest version
        #[arg(short, long)]
        force: bool,

        /// Include pre-release versions (Alpha / Beta / RC)
        #[arg(short = 'p', long)]
        include_prereleases: bool,
    },

    /// Safely uninstall agy-orbit and clean runtime data
    #[command(
        visible_aliases = ["purge"],
        long_about = "Safely uninstall agy-orbit and clean up runtime data, locks, and multi-account storage.\n\
                      Note: Official Google Antigravity credentials in ~/.gemini/ are kept intact."
    )]
    Uninstall {
        /// Automatically confirm uninstallation without interactive prompts
        #[arg(short = 'y', long = "yes")]
        yes: bool,

        /// Preview changes without deleting any files
        #[arg(long)]
        dry_run: bool,

        /// Preserve encrypted multi-account vaults and orbits (~/.agyo/orbits/)
        #[arg(long = "keep-vault", visible_alias = "keep-data")]
        keep_vault: bool,

        /// Attempt to remove the running executable binary itself
        #[arg(long)]
        delete_self: bool,
    },

    /// Internal fast query for shell completions (outputs orbit names only)
    #[command(hide = true, name = "__complete-orbits")]
    CompleteOrbits,
}
