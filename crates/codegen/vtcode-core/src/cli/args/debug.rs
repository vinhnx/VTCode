use clap::Subcommand;

/// Inspect and exercise the OS sandbox without starting an agent session.
///
/// Commands run under the sandbox configured for this workspace, so this is a
/// way to verify that a profile blocks what it is supposed to block.
#[derive(Subcommand, Debug, Clone)]
pub enum DebugCommand {
    /// Print the sandbox mechanisms available on this platform.
    Capabilities,

    /// Run a command under macOS Seatbelt (`sandbox-exec`).
    Seatbelt {
        /// Command and arguments to run (all following values are passed through).
        #[arg(
            long = "cmd",
            value_name = "COMMAND",
            num_args = 1..,
            allow_hyphen_values = true,
            required = true
        )]
        command: Vec<String>,
    },

    /// Run a command under Linux Landlock plus seccomp.
    Landlock {
        /// Command and arguments to run (all following values are passed through).
        #[arg(
            long = "cmd",
            value_name = "COMMAND",
            num_args = 1..,
            allow_hyphen_values = true,
            required = true
        )]
        command: Vec<String>,
    },
}
