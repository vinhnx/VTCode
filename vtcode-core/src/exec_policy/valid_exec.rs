use serde::Serialize;

use super::arg_type::ArgType;
use super::error::Result;

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct ValidExec {
    pub program: String,
    pub flags: Vec<MatchedFlag>,
    pub opts: Vec<MatchedOpt>,
    pub args: Vec<MatchedArg>,
    pub system_path: Vec<String>,
}

impl ValidExec {
    pub fn new(program: &str, args: Vec<MatchedArg>, system_path: &[&str]) -> Self {
        Self {
            program: program.to_string(),
            flags: vec![],
            opts: vec![],
            args,
            system_path: system_path.iter().map(|&s| s.to_string()).collect(),
        }
    }

    pub fn might_write_files(&self) -> bool {
        self.opts.iter().any(|opt| opt.r#type.might_write_file())
            || self.args.iter().any(|opt| opt.r#type.might_write_file())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MatchedArg {
    pub index: usize,
    pub r#type: ArgType,
    pub value: String,
}

impl MatchedArg {
    pub fn new(index: usize, r#type: ArgType, value: &str) -> Result<Self> {
        r#type.validate(value)?;
        Ok(Self {
            index,
            r#type,
            value: value.to_string(),
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MatchedOpt {
    pub name: String,
    pub value: String,
    pub r#type: ArgType,
}

impl MatchedOpt {
    pub fn new(name: &str, value: &str, r#type: ArgType) -> Result<Self> {
        r#type.validate(value)?;
        Ok(Self {
            name: name.to_string(),
            value: value.to_string(),
            r#type,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct MatchedFlag {
    pub name: String,
}

impl MatchedFlag {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}
