//! COM Object Context Analyzer (Phase 6.2)
//!
//! Analyzes PowerShell scripts for dangerous COM object instantiation patterns.
//! Provides context-aware detection of:
//! - COM object creation methods
//! - Dangerous object types (WScript.Shell, Shell.Application, etc.)
//! - Method invocation patterns
//! - Execution scope and privilege context

use std::collections::HashMap;

/// COM object risk level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ComRiskLevel {
    Safe = 0,
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

impl std::fmt::Display for ComRiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Safe => write!(f, "SAFE"),
            Self::Low => write!(f, "LOW"),
            Self::Medium => write!(f, "MEDIUM"),
            Self::High => write!(f, "HIGH"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// COM object metadata
#[derive(Debug, Clone)]
pub struct ComObjectInfo {
    pub prog_id: &'static str,
    pub risk_level: ComRiskLevel,
    pub dangerous_methods: &'static [&'static str],
    pub description: &'static str,
}

/// Map of dangerous COM objects
pub fn get_com_objects() -> HashMap<&'static str, ComObjectInfo> {
    let mut m = HashMap::new();

    // ──── CRITICAL: Shell Execution & Administration ────
    m.insert(
        "wscript.shell",
        ComObjectInfo {
            prog_id: "WScript.Shell",
            risk_level: ComRiskLevel::Critical,
            dangerous_methods: &[
                "exec",
                "run",
                "createobject",
                "sendkeys",
                "popup",
            ],
            description: "Allows arbitrary process execution and shell operations",
        },
    );

    m.insert(
        "shell.application",
        ComObjectInfo {
            prog_id: "Shell.Application",
            risk_level: ComRiskLevel::Critical,
            dangerous_methods: &[
                "shellexecute",
                "open",
                "createshelllink",
            ],
            description: "Shell context for file operations and process execution",
        },
    );

    // ──── HIGH: System Administration ────
    m.insert(
        "activedirectory.adsiobject",
        ComObjectInfo {
            prog_id: "ActiveDirectory.ADSIObject",
            risk_level: ComRiskLevel::High,
            dangerous_methods: &[
                "setinfo",
                "createobject",
                "bindtoobject",
            ],
            description: "Active Directory access for privilege escalation",
        },
    );

    m.insert(
        "activexobject",
        ComObjectInfo {
            prog_id: "ActiveXObject",
            risk_level: ComRiskLevel::High,
            dangerous_methods: &[],
            description: "Generic COM object instantiation (context-dependent)",
        },
    );

    m.insert(
        "microsoft.xmldom",
        ComObjectInfo {
            prog_id: "Microsoft.XMLDOM",
            risk_level: ComRiskLevel::High,
            dangerous_methods: &[
                "load",
                "loadxml",
                "selectnodes",
            ],
            description: "XML processing that can load external content",
        },
    );

    m.insert(
        "microsoft.xmlhttp",
        ComObjectInfo {
            prog_id: "MSXML2.XMLHTTP",
            risk_level: ComRiskLevel::High,
            dangerous_methods: &[
                "open",
                "send",
                "responsetext",
            ],
            description: "HTTP requests for downloading code/data",
        },
    );

    m.insert(
        "scripting.filesystemobject",
        ComObjectInfo {
            prog_id: "Scripting.FileSystemObject",
            risk_level: ComRiskLevel::High,
            dangerous_methods: &[
                "createtextfile",
                "createfolder",
                "copyfile",
                "movefile",
                "deletefile",
            ],
            description: "File system operations without PowerShell restrictions",
        },
    );

    m.insert(
        "microsoft.update.session",
        ComObjectInfo {
            prog_id: "Microsoft.Update.Session",
            risk_level: ComRiskLevel::High,
            dangerous_methods: &[
                "createsearcher",
                "createupdatedownloader",
            ],
            description: "Windows Update enumeration for security bypass",
        },
    );

    // ──── HIGH: Network & Credential Access ────
    m.insert(
        "internet.session",
        ComObjectInfo {
            prog_id: "Internet.Session",
            risk_level: ComRiskLevel::High,
            dangerous_methods: &[
                "getfile",
                "openurl",
            ],
            description: "Direct internet access for file download",
        },
    );

    m.insert(
        "winhttprequest.5.1",
        ComObjectInfo {
            prog_id: "WinHttpRequest.5.1",
            risk_level: ComRiskLevel::High,
            dangerous_methods: &[
                "open",
                "send",
                "responsetext",
            ],
            description: "HTTP requests with credential support",
        },
    );

    // ──── MEDIUM: WMI & System Info ────
    m.insert(
        "winmgmts",
        ComObjectInfo {
            prog_id: "WinMgmts",
            risk_level: ComRiskLevel::Medium,
            dangerous_methods: &[
                "execmethod",
                "get",
                "instancesof",
            ],
            description: "WMI access for system manipulation",
        },
    );

    m.insert(
        "wbemscripting.swbemlocator",
        ComObjectInfo {
            prog_id: "WbemScripting.SWbemLocator",
            risk_level: ComRiskLevel::Medium,
            dangerous_methods: &[
                "connectserver",
            ],
            description: "WMI locator for remote system access",
        },
    );

    // ──── MEDIUM: Office Automation ────
    m.insert(
        "excel.application",
        ComObjectInfo {
            prog_id: "Excel.Application",
            risk_level: ComRiskLevel::Medium,
            dangerous_methods: &[
                "run",
                "activatex",
            ],
            description: "Excel macro execution via COM",
        },
    );

    m.insert(
        "word.application",
        ComObjectInfo {
            prog_id: "Word.Application",
            risk_level: ComRiskLevel::Medium,
            dangerous_methods: &[
                "run",
            ],
            description: "Word macro execution via COM",
        },
    );

    m.insert(
        "powerpoint.application",
        ComObjectInfo {
            prog_id: "PowerPoint.Application",
            risk_level: ComRiskLevel::Medium,
            dangerous_methods: &[
                "run",
            ],
            description: "PowerPoint macro execution via COM",
        },
    );

    m.insert(
        "outlook.application",
        ComObjectInfo {
            prog_id: "Outlook.Application",
            risk_level: ComRiskLevel::Medium,
            dangerous_methods: &[
                "createitem",
                "send",
            ],
            description: "Outlook access for email and credential theft",
        },
    );

    // ──── LOW: Safe COM Objects ────
    m.insert(
        "system.diagnostics.process",
        ComObjectInfo {
            prog_id: "System.Diagnostics.Process",
            risk_level: ComRiskLevel::Low,
            dangerous_methods: &[],
            description: ".NET Process class (can execute commands)",
        },
    );

    m
}

/// COM object analysis context
#[derive(Debug, Clone)]
pub struct ComObjectContext {
    pub object_name: String,
    pub creation_method: String,
    pub risk_level: ComRiskLevel,
    pub dangerous_methods_used: Vec<String>,
    pub is_in_scriptblock: bool,
    pub has_invoke_expression: bool,
}

/// Analyzer for COM object patterns
pub struct ComObjectAnalyzer;

impl ComObjectAnalyzer {
    /// Get COM object metadata
    pub fn get_object_info(prog_id: &str) -> Option<ComObjectInfo> {
        let objects = get_com_objects();
        objects.get(&prog_id.to_lowercase().as_str()).cloned()
    }

    /// Analyze COM object instantiation in script
    pub fn analyze_instantiation(script: &str) -> Vec<ComObjectContext> {
        let mut contexts = Vec::new();
        let objects = get_com_objects();
        let script_lower = script.to_lowercase();

        for (prog_id, info) in objects.iter() {
            if script_lower.contains(prog_id) {
                let dangerous_methods: Vec<String> = info
                    .dangerous_methods
                    .iter()
                    .filter(|method| {
                        script_lower.contains(&format!(".{}", method))
                            || script_lower.contains(&format!(" {}", method))
                    })
                    .map(|s| s.to_string())
                    .collect();

                let is_in_scriptblock =
                    script_lower.contains("-scriptblock") || script_lower.contains("&{");

                let has_invoke_expression =
                    script_lower.contains("invoke-expression") || script_lower.contains("iex");

                contexts.push(ComObjectContext {
                    object_name: prog_id.to_string(),
                    creation_method: identify_creation_method(script, prog_id),
                    risk_level: info.risk_level,
                    dangerous_methods_used: dangerous_methods,
                    is_in_scriptblock,
                    has_invoke_expression,
                });
            }
        }

        contexts
    }

    /// Check if any critical COM objects are instantiated
    pub fn has_critical_com_objects(script: &str) -> bool {
        Self::analyze_instantiation(script)
            .iter()
            .any(|ctx| ctx.risk_level == ComRiskLevel::Critical)
    }

    /// Get maximum risk level in script
    pub fn get_max_risk_level(script: &str) -> ComRiskLevel {
        Self::analyze_instantiation(script)
            .iter()
            .map(|ctx| ctx.risk_level)
            .max()
            .unwrap_or(ComRiskLevel::Safe)
    }

    /// Check if COM usage is dangerous (has critical objects or ivocation patterns)
    pub fn is_dangerous_com_usage(script: &str) -> bool {
        let contexts = Self::analyze_instantiation(script);
        contexts.iter().any(|ctx| {
            ctx.risk_level >= ComRiskLevel::High
                && (ctx.has_invoke_expression || !ctx.dangerous_methods_used.is_empty())
        })
    }
}

/// Identify COM object creation method
fn identify_creation_method(script: &str, prog_id: &str) -> String {
    let script_lower = script.to_lowercase();
    let prog_id_lower = prog_id.to_lowercase();

    if script_lower.contains(&format!("new-object -comobject {}", prog_id_lower)) {
        "New-Object -ComObject".to_string()
    } else if script_lower.contains(&format!("[activexobject]'{}'", prog_id_lower)) {
        "[ActiveXObject] cast".to_string()
    } else if script_lower.contains(&format!("createobject('{}')", prog_id_lower)) {
        "CreateObject()".to_string()
    } else if script_lower.contains(&format!("getobject(\"\",'{}')", prog_id_lower)) {
        "GetObject()".to_string()
    } else {
        "Unknown".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_wscript_shell_info() {
        let info = ComObjectAnalyzer::get_object_info("wscript.shell").unwrap();
        assert_eq!(info.risk_level, ComRiskLevel::Critical);
        assert!(info.dangerous_methods.contains(&"exec"));
    }

    #[test]
    fn test_case_insensitive_lookup() {
        assert!(ComObjectAnalyzer::get_object_info("WSCRIPT.SHELL").is_some());
        assert!(ComObjectAnalyzer::get_object_info("WScript.Shell").is_some());
    }

    #[test]
    fn test_analyze_instantiation() {
        let script = r#"
            $shell = New-Object -ComObject WScript.Shell
            $shell.Run("calc.exe")
        "#;
        let contexts = ComObjectAnalyzer::analyze_instantiation(script);
        assert!(!contexts.is_empty());
        assert!(contexts[0].dangerous_methods_used.contains(&"run".to_string()));
    }

    #[test]
    fn test_has_critical_com_objects() {
        let script = "New-Object -ComObject WScript.Shell";
        assert!(ComObjectAnalyzer::has_critical_com_objects(script));

        let safe_script = "Get-Item -Path C:\\";
        assert!(!ComObjectAnalyzer::has_critical_com_objects(safe_script));
    }

    #[test]
    fn test_get_max_risk_level() {
        let script = r#"
            $shell = New-Object -ComObject WScript.Shell
            $xml = New-Object -ComObject Microsoft.XMLDOM
        "#;
        let max_risk = ComObjectAnalyzer::get_max_risk_level(script);
        assert!(max_risk >= ComRiskLevel::Critical);
    }

    #[test]
    fn test_is_dangerous_com_usage() {
        let dangerous = "New-Object -ComObject WScript.Shell | % { $_.Run('cmd.exe') }";
        assert!(ComObjectAnalyzer::is_dangerous_com_usage(dangerous));

        let safe = "Get-Item -Path C:\\";
        assert!(!ComObjectAnalyzer::is_dangerous_com_usage(safe));
    }

    #[test]
    fn test_creation_method_detection() {
        let method1 = identify_creation_method("New-Object -ComObject WScript.Shell", "wscript.shell");
        assert_eq!(method1, "New-Object -ComObject");

        let method2 = identify_creation_method("[activexobject]'WScript.Shell'", "wscript.shell");
        assert_eq!(method2, "[ActiveXObject] cast");
    }

    #[test]
    fn test_com_risk_level_ordering() {
        assert!(ComRiskLevel::Critical > ComRiskLevel::High);
        assert!(ComRiskLevel::High > ComRiskLevel::Medium);
        assert!(ComRiskLevel::Medium > ComRiskLevel::Low);
        assert!(ComRiskLevel::Low > ComRiskLevel::Safe);
    }

    #[test]
    fn test_scripting_filesystemobject_detection() {
        let script = "New-Object -ComObject Scripting.FileSystemObject";
        let contexts = ComObjectAnalyzer::analyze_instantiation(script);
        assert!(!contexts.is_empty());
        assert_eq!(contexts[0].risk_level, ComRiskLevel::High);
    }
}
