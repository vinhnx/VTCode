#![allow(clippy::type_complexity)]
#![allow(clippy::too_many_arguments)]

mod arg_matcher;
mod arg_resolver;
mod arg_type;
mod error;
mod exec_call;
mod execv_checker;
mod opt;
mod policy;
mod policy_parser;
mod program;
mod sed_command;
mod valid_exec;

pub mod manager;

pub use arg_matcher::ArgMatcher;
pub use arg_resolver::PositionalArg;
pub use arg_type::ArgType;
pub use error::{Error, Result};
pub use exec_call::ExecCall;
pub use execv_checker::ExecvChecker;
pub use manager::{ExecPolicyManager, ExecPolicyReport, ExecPolicyVerdict};
pub use opt::Opt;
pub use policy::Policy;
pub use policy_parser::PolicyParser;
pub use program::{
    Forbidden, MatchedExec, NegativeExamplePassedCheck, PositiveExampleFailedCheck, ProgramSpec,
};
pub use sed_command::parse_sed_command;
pub use valid_exec::{MatchedArg, MatchedFlag, MatchedOpt, ValidExec};

const DEFAULT_POLICY: &str = include_str!("default.policy");

pub fn parse_default_policy() -> starlark::Result<Policy> {
    let parser = PolicyParser::new("#default", DEFAULT_POLICY);
    parser.parse()
}

pub fn default_execv_checker() -> anyhow::Result<ExecvChecker> {
    let policy = parse_default_policy().map_err(|error| anyhow::anyhow!(error))?;
    Ok(ExecvChecker::new(policy))
}
