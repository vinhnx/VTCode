use super::error::{Error, Result};

pub fn parse_sed_command(sed_command: &str) -> Result<()> {
    if let Some(stripped) = sed_command.strip_suffix("p")
        && let Some((first, rest)) = stripped.split_once(",")
        && first.parse::<u64>().is_ok()
        && rest.parse::<u64>().is_ok()
    {
        return Ok(());
    }

    Err(Error::SedCommandNotProvablySafe {
        command: sed_command.to_string(),
    })
}
