use std::collections::{HashMap, HashSet};

use serde::Serialize;

use super::arg_matcher::ArgMatcher;
use super::arg_resolver::{PositionalArg, resolve_observed_args_with_patterns};
use super::arg_type::ArgType;
use super::error::{Error, Result};
use super::exec_call::ExecCall;
use super::opt::{Opt, OptMeta};
use super::valid_exec::{MatchedFlag, MatchedOpt, ValidExec};

#[derive(Clone, Debug)]
pub struct ProgramSpec {
    pub program: String,
    pub system_path: Vec<String>,
    pub option_bundling: bool,
    pub combined_format: bool,
    pub allowed_options: HashMap<String, Opt>,
    pub arg_patterns: Vec<ArgMatcher>,
    forbidden: Option<String>,
    required_options: HashSet<String>,
    should_match: Vec<Vec<String>>,
    should_not_match: Vec<Vec<String>>,
}

impl ProgramSpec {
    pub fn new(
        program: String,
        system_path: Vec<String>,
        option_bundling: bool,
        combined_format: bool,
        allowed_options: HashMap<String, Opt>,
        arg_patterns: Vec<ArgMatcher>,
        forbidden: Option<String>,
        should_match: Vec<Vec<String>>,
        should_not_match: Vec<Vec<String>>,
    ) -> Self {
        let required_options = allowed_options
            .iter()
            .filter_map(|(name, opt)| {
                if opt.required {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        Self {
            program,
            system_path,
            option_bundling,
            combined_format,
            allowed_options,
            arg_patterns,
            forbidden,
            required_options,
            should_match,
            should_not_match,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum MatchedExec {
    Match { exec: ValidExec },
    Forbidden { cause: Forbidden, reason: String },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub enum Forbidden {
    Program {
        program: String,
        exec_call: ExecCall,
    },
    Arg {
        arg: String,
        exec_call: ExecCall,
    },
    Exec {
        exec: ValidExec,
    },
}

impl ProgramSpec {
    pub fn check(&self, exec_call: &ExecCall) -> Result<MatchedExec> {
        let mut expecting_option_value: Option<(String, ArgType)> = None;
        let mut args = Vec::<PositionalArg>::new();
        let mut matched_flags = Vec::<MatchedFlag>::new();
        let mut matched_opts = Vec::<MatchedOpt>::new();

        for (index, arg) in exec_call.args.iter().enumerate() {
            if let Some(expected) = expecting_option_value {
                let (name, arg_type) = expected;
                if arg.starts_with("-") {
                    return Err(Error::OptionFollowedByOptionInsteadOfValue {
                        program: self.program.clone(),
                        option: name,
                        value: arg.clone(),
                    });
                }

                matched_opts.push(MatchedOpt::new(&name, arg, arg_type)?);
                expecting_option_value = None;
            } else if arg == "--" {
                return Err(Error::DoubleDashNotSupportedYet {
                    program: self.program.clone(),
                });
            } else if arg.starts_with("-") {
                match self.allowed_options.get(arg) {
                    Some(opt) => match &opt.meta {
                        OptMeta::Flag => {
                            matched_flags.push(MatchedFlag { name: arg.clone() });
                            continue;
                        }
                        OptMeta::Value(arg_type) => {
                            expecting_option_value = Some((arg.clone(), arg_type.clone()));
                            continue;
                        }
                    },
                    None => {}
                }

                return Err(Error::UnknownOption {
                    program: self.program.clone(),
                    option: arg.clone(),
                });
            } else {
                args.push(PositionalArg {
                    index,
                    value: arg.clone(),
                });
            }
        }

        if let Some(expected) = expecting_option_value {
            let (name, _arg_type) = expected;
            return Err(Error::OptionMissingValue {
                program: self.program.clone(),
                option: name,
            });
        }

        let matched_args =
            resolve_observed_args_with_patterns(&self.program, args, &self.arg_patterns)?;

        let matched_opt_names: HashSet<String> = matched_opts
            .iter()
            .map(|opt| opt.name().to_string())
            .collect();
        if !matched_opt_names.is_superset(&self.required_options) {
            let mut options = self
                .required_options
                .difference(&matched_opt_names)
                .map(String::from)
                .collect::<Vec<_>>();
            options.sort();
            return Err(Error::MissingRequiredOptions {
                program: self.program.clone(),
                options,
            });
        }

        let exec = ValidExec {
            program: self.program.clone(),
            flags: matched_flags,
            opts: matched_opts,
            args: matched_args,
            system_path: self.system_path.clone(),
        };
        match &self.forbidden {
            Some(reason) => Ok(MatchedExec::Forbidden {
                cause: Forbidden::Exec { exec },
                reason: reason.clone(),
            }),
            None => Ok(MatchedExec::Match { exec }),
        }
    }

    pub fn verify_should_match_list(&self) -> Vec<PositiveExampleFailedCheck> {
        let mut violations = Vec::new();
        for good in &self.should_match {
            let exec_call = ExecCall {
                program: self.program.clone(),
                args: good.clone(),
            };
            match self.check(&exec_call) {
                Ok(MatchedExec::Match { .. }) => {}
                Ok(MatchedExec::Forbidden { reason, .. }) => violations
                    .push(PositiveExampleFailedCheck::MatchedForbiddenRule { exec_call, reason }),
                Err(error) => violations
                    .push(PositiveExampleFailedCheck::RejectedExecCall { exec_call, error }),
            }
        }
        violations
    }

    pub fn verify_should_not_match_list(&self) -> Vec<NegativeExamplePassedCheck> {
        let mut violations = Vec::new();
        for bad in &self.should_not_match {
            let exec_call = ExecCall {
                program: self.program.clone(),
                args: bad.clone(),
            };
            match self.check(&exec_call) {
                Ok(MatchedExec::Match { exec }) => {
                    violations.push(NegativeExamplePassedCheck::Match { exec_call, exec })
                }
                Ok(MatchedExec::Forbidden { .. }) => {}
                Err(error) => {
                    violations.push(NegativeExamplePassedCheck::Error { exec_call, error })
                }
            }
        }
        violations
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub enum PositiveExampleFailedCheck {
    MatchedForbiddenRule { exec_call: ExecCall, reason: String },
    RejectedExecCall { exec_call: ExecCall, error: Error },
}

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub enum NegativeExamplePassedCheck {
    Match {
        exec_call: ExecCall,
        exec: ValidExec,
    },
    Error {
        exec_call: ExecCall,
        error: Error,
    },
}
