//! Shared slash-command grammar used by the CLI and desktop client.

/// Human-readable catalog of the operations exposed by the WebMCP bridge.
pub const WEBMCP_TOOLS_HELP: &str = "WebMCP tools: workspace.list_files, workspace.read_file, patch.propose, patch.apply, checks.run, patch.revert, turn.request, cancel";

/// Configuration guidance for standalone headless WebMCP workspace roots.
pub const WEBMCP_ROOTS_HELP: &str =
    "Headless WebMCP roots are configured with [webmcp].allowed_roots or --allowed-root.";

/// A parsed `/webmcp` slash command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebmcpSlashCommand {
    /// Show the active WebMCP bridge status.
    Status,
    /// Show WebMCP command help.
    Help,
    /// List the supported WebMCP tools.
    Tools,
    /// Describe the configured headless WebMCP workspace roots.
    Roots,
    /// Show usage for the `connect` or `pair` command.
    ConnectHelp,
    /// Pair a browser origin.
    Connect {
        /// Exact browser origin.
        origin: String,
        /// Revoke existing pairings before issuing a new pairing code.
        replace: bool,
    },
    /// Stop the bridge or revoke its in-memory pairings.
    Unpair,
}

/// Why a WebMCP slash command could not be parsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebmcpSlashCommandParseError {
    /// The verb is not a recognized WebMCP command or has unsupported trailing arguments.
    UnknownCommand,
    /// A `connect` or `pair` command has malformed origin or replacement arguments.
    InvalidConnectionArguments,
}

/// Parse the arguments written after `/webmcp`.
///
/// The empty argument string and `status` both select the status command. `pair` aliases `connect`,
/// and `stop` aliases `unpair` so both frontends accept the same command vocabulary.
///
/// # Errors
/// Returns [`WebmcpSlashCommandParseError::UnknownCommand`] for an unsupported verb and
/// [`WebmcpSlashCommandParseError::InvalidConnectionArguments`] for malformed pairing options.
pub fn parse_webmcp_slash_command(arguments: &str) -> Result<WebmcpSlashCommand, WebmcpSlashCommandParseError> {
    let arguments = arguments.trim();
    if arguments.is_empty() {
        return Ok(WebmcpSlashCommand::Status);
    }

    let (verb, tail) = arguments
        .split_once(char::is_whitespace)
        .map_or((arguments, ""), |(verb, tail)| (verb, tail.trim()));

    match verb {
        "status" if tail.is_empty() => Ok(WebmcpSlashCommand::Status),
        "help" if tail.is_empty() => Ok(WebmcpSlashCommand::Help),
        "tools" if tail.is_empty() => Ok(WebmcpSlashCommand::Tools),
        "roots" if tail.is_empty() => Ok(WebmcpSlashCommand::Roots),
        "unpair" | "stop" if tail.is_empty() => Ok(WebmcpSlashCommand::Unpair),
        "connect" | "pair" => parse_connection_arguments(tail),
        _ => Err(WebmcpSlashCommandParseError::UnknownCommand),
    }
}

fn parse_connection_arguments(arguments: &str) -> Result<WebmcpSlashCommand, WebmcpSlashCommandParseError> {
    if arguments.is_empty() {
        return Ok(WebmcpSlashCommand::ConnectHelp);
    }

    let mut tokens = arguments.split_whitespace();
    let mut origin = None;
    let mut replace = false;

    while let Some(token) = tokens.next() {
        match token {
            "--replace" if !replace => replace = true,
            "--origin" if origin.is_none() => {
                let Some(value) = tokens.next() else {
                    return Err(WebmcpSlashCommandParseError::InvalidConnectionArguments);
                };
                if value.starts_with('-') {
                    return Err(WebmcpSlashCommandParseError::InvalidConnectionArguments);
                }
                origin = Some(value);
            }
            value if origin.is_none() && !value.starts_with('-') => origin = Some(value),
            _ => return Err(WebmcpSlashCommandParseError::InvalidConnectionArguments),
        }
    }

    let Some(origin) = origin else {
        return Err(WebmcpSlashCommandParseError::InvalidConnectionArguments);
    };

    Ok(WebmcpSlashCommand::Connect { origin: origin.to_owned(), replace })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_status_help_and_webmcp_catalog_commands() {
        assert_eq!(parse_webmcp_slash_command(""), Ok(WebmcpSlashCommand::Status));
        assert_eq!(parse_webmcp_slash_command(" status "), Ok(WebmcpSlashCommand::Status));
        assert_eq!(parse_webmcp_slash_command("help"), Ok(WebmcpSlashCommand::Help));
        assert_eq!(parse_webmcp_slash_command("tools"), Ok(WebmcpSlashCommand::Tools));
        assert_eq!(parse_webmcp_slash_command("roots"), Ok(WebmcpSlashCommand::Roots));
    }

    #[test]
    fn parses_connect_aliases_and_options_in_either_order() {
        assert_eq!(
            parse_webmcp_slash_command("connect https://example.test"),
            Ok(WebmcpSlashCommand::Connect {
                origin: "https://example.test".into(),
                replace: false,
            })
        );
        assert_eq!(
            parse_webmcp_slash_command("pair --replace --origin http://localhost:5173"),
            Ok(WebmcpSlashCommand::Connect {
                origin: "http://localhost:5173".into(),
                replace: true,
            })
        );
        assert_eq!(
            parse_webmcp_slash_command("connect --origin http://localhost:5173 --replace"),
            Ok(WebmcpSlashCommand::Connect {
                origin: "http://localhost:5173".into(),
                replace: true,
            })
        );
        assert_eq!(parse_webmcp_slash_command("pair"), Ok(WebmcpSlashCommand::ConnectHelp));
    }

    #[test]
    fn parses_both_disconnect_verbs() {
        assert_eq!(parse_webmcp_slash_command("unpair"), Ok(WebmcpSlashCommand::Unpair));
        assert_eq!(parse_webmcp_slash_command("stop"), Ok(WebmcpSlashCommand::Unpair));
    }

    #[test]
    fn rejects_ambiguous_or_malformed_commands() {
        for input in [
            "status extra",
            "unknown",
            "pair https://a.test https://b.test",
            "pair --unknown https://a.test",
            "pair --replace --replace https://a.test",
            "pair --origin --replace",
            "connect --replace",
        ] {
            assert!(parse_webmcp_slash_command(input).is_err(), "accepted {input:?}");
        }
    }
}
