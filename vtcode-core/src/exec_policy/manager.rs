use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use super::default_execv_checker;
use super::error::Error;
use super::exec_call::ExecCall;
use super::execv_checker::ExecvChecker;
use super::program::{Forbidden, MatchedExec};
use super::valid_exec::ValidExec;

#[derive(Debug, Clone)]
pub struct ExecPolicyManager {
    checker: ExecvChecker,
    workspace_root: PathBuf,
    readable_roots: Vec<PathBuf>,
    writeable_roots: Vec<PathBuf>,
}

impl ExecPolicyManager {
    pub fn new(workspace_root: PathBuf) -> Result<Self> {
        let checker =
            default_execv_checker().with_context(|| "failed to load default exec policy")?;
        let canonical_root = workspace_root
            .canonicalize()
            .unwrap_or(workspace_root.clone());

        let mut readable_roots = vec![canonical_root.clone()];
        let mut writeable_roots = vec![canonical_root.clone()];

        let temp_dir = std::env::temp_dir()
            .canonicalize()
            .unwrap_or_else(|_| std::env::temp_dir());
        if !readable_roots.contains(&temp_dir) {
            readable_roots.push(temp_dir.clone());
        }
        if !writeable_roots.contains(&temp_dir) {
            writeable_roots.push(temp_dir);
        }

        Ok(Self {
            checker,
            workspace_root: canonical_root,
            readable_roots,
            writeable_roots,
        })
    }

    pub fn assess(&self, command: &[String], working_dir: &Path) -> ExecPolicyReport {
        if command.is_empty() {
            return ExecPolicyReport::unverified(Some("command had no program".to_string()), None);
        }

        let exec_call = ExecCall {
            program: command[0].clone(),
            args: command[1..].to_vec(),
        };

        match self.checker.r#match(&exec_call) {
            Ok(MatchedExec::Match { exec }) => self.handle_match(exec, working_dir),
            Ok(MatchedExec::Forbidden { cause, reason }) => {
                ExecPolicyReport::forbidden(Some(reason), Some(cause), None)
            }
            Err(Error::NoSpecForProgram { .. }) => ExecPolicyReport::not_covered(),
            Err(error) => ExecPolicyReport::unverified(None, Some(error)),
        }
    }

    fn handle_match(&self, exec: ValidExec, working_dir: &Path) -> ExecPolicyReport {
        let cwd = self.resolve_cwd(working_dir);
        let cwd_os = Some(cwd.clone().into_os_string());

        let mut readable = self.readable_roots.clone();
        if !readable.iter().any(|path| path == &cwd) {
            readable.push(cwd.clone());
        }

        let mut writeable = self.writeable_roots.clone();
        if !writeable.iter().any(|path| path == &cwd) {
            writeable.push(cwd.clone());
        }

        let exec_for_report = exec.clone();
        match self.checker.check(exec, &cwd_os, &readable, &writeable) {
            Ok(program) => {
                if exec_for_report.might_write_files() {
                    ExecPolicyReport::needs_review(Some(program), exec_for_report)
                } else {
                    ExecPolicyReport::safe(Some(program), exec_for_report)
                }
            }
            Err(error) => ExecPolicyReport::forbidden(None, None, Some(error)),
        }
    }

    fn resolve_cwd(&self, working_dir: &Path) -> PathBuf {
        let joined = if working_dir.is_absolute() {
            working_dir.to_path_buf()
        } else {
            self.workspace_root.join(working_dir)
        };
        joined.canonicalize().unwrap_or(joined)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecPolicyVerdict {
    Safe,
    NeedsReview,
    Forbidden,
    Unverified,
    NotCovered,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecPolicyReport {
    pub verdict: ExecPolicyVerdict,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical_program: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exec: Option<ValidExec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub forbidden_cause: Option<Forbidden>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<Error>,
}

impl ExecPolicyReport {
    pub fn safe(canonical_program: Option<String>, exec: ValidExec) -> Self {
        Self {
            verdict: ExecPolicyVerdict::Safe,
            reason: None,
            canonical_program,
            exec: Some(exec),
            forbidden_cause: None,
            error: None,
        }
    }

    pub fn needs_review(canonical_program: Option<String>, exec: ValidExec) -> Self {
        Self {
            verdict: ExecPolicyVerdict::NeedsReview,
            reason: Some("command may write to disk".to_string()),
            canonical_program,
            exec: Some(exec),
            forbidden_cause: None,
            error: None,
        }
    }

    pub fn forbidden(
        reason: Option<String>,
        cause: Option<Forbidden>,
        error: Option<Error>,
    ) -> Self {
        Self {
            verdict: ExecPolicyVerdict::Forbidden,
            reason,
            canonical_program: None,
            exec: match &cause {
                Some(Forbidden::Exec { exec }) => Some(exec.clone()),
                _ => None,
            },
            forbidden_cause: cause,
            error,
        }
    }

    pub fn unverified(reason: Option<String>, error: Option<Error>) -> Self {
        Self {
            verdict: ExecPolicyVerdict::Unverified,
            reason,
            canonical_program: None,
            exec: None,
            forbidden_cause: None,
            error,
        }
    }

    pub fn not_covered() -> Self {
        Self {
            verdict: ExecPolicyVerdict::NotCovered,
            reason: None,
            canonical_program: None,
            exec: None,
            forbidden_cause: None,
            error: None,
        }
    }
}
