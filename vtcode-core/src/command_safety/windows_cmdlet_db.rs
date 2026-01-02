//! PowerShell Cmdlet Database with Severity Levels (Phase 6.1)
//!
//! Comprehensive database of dangerous PowerShell cmdlets organized by severity
//! and purpose, enabling fine-grained control over cmdlet execution.
//!
//! Severity levels:
//! - CRITICAL: System compromise, data destruction, privilege escalation
//! - HIGH: Code execution, arbitrary file operations, network access
//! - MEDIUM: Registry modification, process management, limited file ops
//! - LOW: Information gathering, configuration changes (relatively safe)

use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Severity level of a cmdlet
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum CmdletSeverity {
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

impl std::fmt::Display for CmdletSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Low => write!(f, "LOW"),
            Self::Medium => write!(f, "MEDIUM"),
            Self::High => write!(f, "HIGH"),
            Self::Critical => write!(f, "CRITICAL"),
        }
    }
}

/// Cmdlet category for documentation and filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmdletCategory {
    CodeExecution,
    FileOperations,
    ProcessManagement,
    RegistryAccess,
    NetworkOperations,
    ComObject,
    Reflection,
    SystemManagement,
    Credential,
    Encryption,
}

impl std::fmt::Display for CmdletCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CodeExecution => write!(f, "Code Execution"),
            Self::FileOperations => write!(f, "File Operations"),
            Self::ProcessManagement => write!(f, "Process Management"),
            Self::RegistryAccess => write!(f, "Registry Access"),
            Self::NetworkOperations => write!(f, "Network Operations"),
            Self::ComObject => write!(f, "COM Object"),
            Self::Reflection => write!(f, "Reflection"),
            Self::SystemManagement => write!(f, "System Management"),
            Self::Credential => write!(f, "Credential"),
            Self::Encryption => write!(f, "Encryption"),
        }
    }
}

/// Cmdlet metadata
#[derive(Debug, Clone)]
pub struct CmdletInfo {
    pub name: &'static str,
    pub severity: CmdletSeverity,
    pub category: CmdletCategory,
    pub description: &'static str,
    pub dangerous_patterns: &'static [&'static str],
}

/// Comprehensive cmdlet database
pub static DANGEROUS_CMDLETS: Lazy<HashMap<&'static str, CmdletInfo>> =
    Lazy::new(|| {
        let mut m = HashMap::new();

        // ──── CRITICAL: System Compromise ────
        m.insert("invoke-expression", CmdletInfo {
            name: "Invoke-Expression",
            severity: CmdletSeverity::Critical,
            category: CmdletCategory::CodeExecution,
            description: "Executes PowerShell code in strings, highest privilege escalation risk",
            dangerous_patterns: &["iex", "invoke-expression", "& $"],
        });

        m.insert(
            "invoke-command",
            CmdletInfo {
                name: "Invoke-Command",
                severity: CmdletSeverity::Critical,
                category: CmdletCategory::CodeExecution,
                description: "Executes scripts on remote or local machines",
                dangerous_patterns: &["invoke-command", "-scriptblock"],
            },
        );

        m.insert(
            "new-service",
            CmdletInfo {
                name: "New-Service",
                severity: CmdletSeverity::Critical,
                category: CmdletCategory::SystemManagement,
                description: "Creates new Windows service with arbitrary binary execution",
                dangerous_patterns: &["new-service", "-binarypath"],
            },
        );

        m.insert(
            "remove-item",
            CmdletInfo {
                name: "Remove-Item",
                severity: CmdletSeverity::Critical,
                category: CmdletCategory::FileOperations,
                description: "Deletes files/folders recursively with force flag",
                dangerous_patterns: &["remove-item", "-force", "-recurse"],
            },
        );

        m.insert(
            "stop-process",
            CmdletInfo {
                name: "Stop-Process",
                severity: CmdletSeverity::Critical,
                category: CmdletCategory::ProcessManagement,
                description: "Terminates critical system processes",
                dangerous_patterns: &["stop-process", "-force"],
            },
        );

        // ──── HIGH: Privilege Escalation & Code Execution ────
        m.insert(
            "get-wmiobject",
            CmdletInfo {
                name: "Get-WmiObject",
                severity: CmdletSeverity::High,
                category: CmdletCategory::ComObject,
                description: "Access WMI for system manipulation",
                dangerous_patterns: &["get-wmiobject", "win32_process", "create()"],
            },
        );

        m.insert(
            "invoke-wmimethod",
            CmdletInfo {
                name: "Invoke-WmiMethod",
                severity: CmdletSeverity::High,
                category: CmdletCategory::ComObject,
                description: "Invokes WMI methods for process/service execution",
                dangerous_patterns: &["invoke-wmimethod", "win32_process"],
            },
        );

        m.insert(
            "new-psdrive",
            CmdletInfo {
                name: "New-PSDrive",
                severity: CmdletSeverity::High,
                category: CmdletCategory::FileOperations,
                description: "Maps registry/WMI as drives for modification",
                dangerous_patterns: &["new-psdrive", "-psscope"],
            },
        );

        m.insert(
            "copy-item",
            CmdletInfo {
                name: "Copy-Item",
                severity: CmdletSeverity::High,
                category: CmdletCategory::FileOperations,
                description: "Copies files, can overwrite system binaries",
                dangerous_patterns: &["copy-item", "-path", "system", "-force"],
            },
        );

        m.insert(
            "move-item",
            CmdletInfo {
                name: "Move-Item",
                severity: CmdletSeverity::High,
                category: CmdletCategory::FileOperations,
                description: "Moves/renames files, can replace critical system files",
                dangerous_patterns: &["move-item", "-path", "windows\\"],
            },
        );

        m.insert(
            "set-executionpolicy",
            CmdletInfo {
                name: "Set-ExecutionPolicy",
                severity: CmdletSeverity::High,
                category: CmdletCategory::SystemManagement,
                description: "Modifies PowerShell script execution policy",
                dangerous_patterns: &["set-executionpolicy", "bypass", "unrestricted"],
            },
        );

        m.insert(
            "new-item",
            CmdletInfo {
                name: "New-Item",
                severity: CmdletSeverity::High,
                category: CmdletCategory::FileOperations,
                description: "Creates files/registry entries, can inject malware",
                dangerous_patterns: &["new-item", "-itemtype", "file", "windows\\"],
            },
        );

        m.insert(
            "invoke-webrequest",
            CmdletInfo {
                name: "Invoke-WebRequest",
                severity: CmdletSeverity::High,
                category: CmdletCategory::NetworkOperations,
                description: "Downloads from internet, especially if piped to IEX",
                dangerous_patterns: &["invoke-webrequest", "| iex", "| invoke-expression"],
            },
        );

        m.insert(
            "start-process",
            CmdletInfo {
                name: "Start-Process",
                severity: CmdletSeverity::High,
                category: CmdletCategory::ProcessManagement,
                description: "Spawns new processes with elevated privileges",
                dangerous_patterns: &["start-process", "-verb", "runas"],
            },
        );

        // ──── HIGH: Remote Code Execution ────
        m.insert(
            "enable-psremoting",
            CmdletInfo {
                name: "Enable-PSRemoting",
                severity: CmdletSeverity::High,
                category: CmdletCategory::SystemManagement,
                description: "Enables PowerShell remoting for lateral movement",
                dangerous_patterns: &["enable-psremoting", "-force"],
            },
        );

        m.insert(
            "set-item",
            CmdletInfo {
                name: "Set-Item",
                severity: CmdletSeverity::High,
                category: CmdletCategory::FileOperations,
                description: "Modifies file contents/attributes",
                dangerous_patterns: &["set-item", "-path", "system"],
            },
        );

        m.insert(
            "add-type",
            CmdletInfo {
                name: "Add-Type",
                severity: CmdletSeverity::High,
                category: CmdletCategory::CodeExecution,
                description: "Loads .NET assemblies for arbitrary code execution",
                dangerous_patterns: &["add-type", "-assemblypathname", "-typedefinition"],
            },
        );

        // ──── MEDIUM: Registry & Configuration Modification ────
        m.insert(
            "new-itemproperty",
            CmdletInfo {
                name: "New-ItemProperty",
                severity: CmdletSeverity::Medium,
                category: CmdletCategory::RegistryAccess,
                description: "Creates registry entries for persistence",
                dangerous_patterns: &["new-itemproperty", "hklm:", "run"],
            },
        );

        m.insert(
            "set-itemproperty",
            CmdletInfo {
                name: "Set-ItemProperty",
                severity: CmdletSeverity::Medium,
                category: CmdletCategory::RegistryAccess,
                description: "Modifies registry values for configuration hijacking",
                dangerous_patterns: &["set-itemproperty", "hklm:", "environment"],
            },
        );

        m.insert(
            "remove-itemproperty",
            CmdletInfo {
                name: "Remove-ItemProperty",
                severity: CmdletSeverity::Medium,
                category: CmdletCategory::RegistryAccess,
                description: "Removes registry entries, can disable security",
                dangerous_patterns: &["remove-itemproperty", "hklm:", "defender"],
            },
        );

        m.insert(
            "get-process",
            CmdletInfo {
                name: "Get-Process",
                severity: CmdletSeverity::Medium,
                category: CmdletCategory::ProcessManagement,
                description: "Enumerates running processes for reconnaissance",
                dangerous_patterns: &["get-process", "lsass", "svchost"],
            },
        );

        m.insert(
            "get-service",
            CmdletInfo {
                name: "Get-Service",
                severity: CmdletSeverity::Medium,
                category: CmdletCategory::SystemManagement,
                description: "Enumerates services for targeting",
                dangerous_patterns: &["get-service"],
            },
        );

        m.insert(
            "set-service",
            CmdletInfo {
                name: "Set-Service",
                severity: CmdletSeverity::Medium,
                category: CmdletCategory::SystemManagement,
                description: "Modifies service startup/behavior",
                dangerous_patterns: &["set-service", "-startuptype"],
            },
        );

        m.insert(
            "restart-service",
            CmdletInfo {
                name: "Restart-Service",
                severity: CmdletSeverity::Medium,
                category: CmdletCategory::SystemManagement,
                description: "Restarts services for denial of service",
                dangerous_patterns: &["restart-service", "-force"],
            },
        );

        m.insert(
            "write-host",
            CmdletInfo {
                name: "Write-Host",
                severity: CmdletSeverity::Low,
                category: CmdletCategory::SystemManagement,
                description: "Writes output, but can be part of larger exploit",
                dangerous_patterns: &["write-host"],
            },
        );

        // ──── MEDIUM: Credential & Encryption ────
        m.insert(
            "get-credential",
            CmdletInfo {
                name: "Get-Credential",
                severity: CmdletSeverity::Medium,
                category: CmdletCategory::Credential,
                description: "Prompts for credentials, can be used in phishing",
                dangerous_patterns: &["get-credential"],
            },
        );

        m.insert(
            "convertto-securestring",
            CmdletInfo {
                name: "ConvertTo-SecureString",
                severity: CmdletSeverity::Medium,
                category: CmdletCategory::Encryption,
                description: "Converts to secure strings, used in credential theft",
                dangerous_patterns: &["convertto-securestring", "-asplaintext"],
            },
        );

        m.insert(
            "convertfrom-securestring",
            CmdletInfo {
                name: "ConvertFrom-SecureString",
                severity: CmdletSeverity::Medium,
                category: CmdletCategory::Encryption,
                description: "Decrypts secure strings for credential extraction",
                dangerous_patterns: &["convertfrom-securestring"],
            },
        );

        // ──── MEDIUM: Reflection & Assembly Loading ────
        m.insert(
            "get-member",
            CmdletInfo {
                name: "Get-Member",
                severity: CmdletSeverity::Medium,
                category: CmdletCategory::Reflection,
                description: "Enumerates object members for exploitation",
                dangerous_patterns: &["get-member"],
            },
        );

        m.insert(
            "[reflection.assembly]",
            CmdletInfo {
                name: "[Reflection.Assembly]",
                severity: CmdletSeverity::Medium,
                category: CmdletCategory::Reflection,
                description: "Loads assemblies for .NET method invocation",
                dangerous_patterns: &["reflection.assembly", "loadwithpartialname"],
            },
        );

        // ──── LOW: Information Gathering ────
        m.insert(
            "get-childitem",
            CmdletInfo {
                name: "Get-ChildItem",
                severity: CmdletSeverity::Low,
                category: CmdletCategory::FileOperations,
                description: "Lists files/folders for reconnaissance",
                dangerous_patterns: &["get-childitem"],
            },
        );

        m.insert(
            "get-item",
            CmdletInfo {
                name: "Get-Item",
                severity: CmdletSeverity::Low,
                category: CmdletCategory::FileOperations,
                description: "Gets file/registry items for info gathering",
                dangerous_patterns: &["get-item"],
            },
        );

        m.insert(
            "get-content",
            CmdletInfo {
                name: "Get-Content",
                severity: CmdletSeverity::Low,
                category: CmdletCategory::FileOperations,
                description: "Reads file contents, useful with sensitive files",
                dangerous_patterns: &["get-content"],
            },
        );

        m.insert(
            "test-path",
            CmdletInfo {
                name: "Test-Path",
                severity: CmdletSeverity::Low,
                category: CmdletCategory::FileOperations,
                description: "Tests if path exists",
                dangerous_patterns: &["test-path"],
            },
        );

        m.insert(
            "get-itemproperty",
            CmdletInfo {
                name: "Get-ItemProperty",
                severity: CmdletSeverity::Low,
                category: CmdletCategory::RegistryAccess,
                description: "Reads registry values for info gathering",
                dangerous_patterns: &["get-itemproperty"],
            },
        );

        m
    });

/// Query the cmdlet database
pub struct CmdletDatabase;

impl CmdletDatabase {
    /// Check if a cmdlet is dangerous and get its info
    pub fn get_info(cmdlet_name: &str) -> Option<CmdletInfo> {
        DANGEROUS_CMDLETS
            .get(&cmdlet_name.to_lowercase().as_str())
            .cloned()
    }

    /// Get minimum severity to block
    pub fn get_max_allowed_severity() -> CmdletSeverity {
        // Default: block all but Low severity
        CmdletSeverity::Low
    }

    /// Check if cmdlet exceeds allowed severity
    pub fn is_above_threshold(cmdlet_name: &str, max_severity: CmdletSeverity) -> bool {
        match Self::get_info(cmdlet_name) {
            Some(info) => info.severity > max_severity,
            None => false, // Unknown cmdlets are allowed
        }
    }

    /// Get all cmdlets of a specific severity
    pub fn get_by_severity(severity: CmdletSeverity) -> Vec<CmdletInfo> {
        DANGEROUS_CMDLETS
            .values()
            .filter(|info| info.severity == severity)
            .cloned()
            .collect()
    }

    /// Get all cmdlets of a specific category
    pub fn get_by_category(category: CmdletCategory) -> Vec<CmdletInfo> {
        DANGEROUS_CMDLETS
            .values()
            .filter(|info| info.category == category)
            .cloned()
            .collect()
    }

    /// Count cmdlets by severity
    pub fn count_by_severity() -> HashMap<CmdletSeverity, usize> {
        let mut counts = HashMap::new();
        for info in DANGEROUS_CMDLETS.values() {
            *counts.entry(info.severity).or_insert(0) += 1;
        }
        counts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_critical_cmdlets_present() {
        assert!(CmdletDatabase::get_info("invoke-expression").is_some());
        assert!(CmdletDatabase::get_info("Invoke-Expression").is_some()); // case insensitive
        assert!(CmdletDatabase::get_info("invoke-command").is_some());
        assert!(CmdletDatabase::get_info("new-service").is_some());
    }

    #[test]
    fn test_cmdlet_severity_ordering() {
        assert!(CmdletSeverity::Critical > CmdletSeverity::High);
        assert!(CmdletSeverity::High > CmdletSeverity::Medium);
        assert!(CmdletSeverity::Medium > CmdletSeverity::Low);
    }

    #[test]
    fn test_is_above_threshold() {
        // invoke-expression is CRITICAL
        assert!(CmdletDatabase::is_above_threshold(
            "invoke-expression",
            CmdletSeverity::High
        ));
        assert!(CmdletDatabase::is_above_threshold(
            "invoke-expression",
            CmdletSeverity::Medium
        ));
        assert!(!CmdletDatabase::is_above_threshold(
            "invoke-expression",
            CmdletSeverity::Critical
        ));

        // get-content is LOW
        assert!(!CmdletDatabase::is_above_threshold(
            "get-content",
            CmdletSeverity::Low
        ));
    }

    #[test]
    fn test_get_by_severity() {
        let critical = CmdletDatabase::get_by_severity(CmdletSeverity::Critical);
        assert!(!critical.is_empty());
        assert!(
            critical
                .iter()
                .all(|c| c.severity == CmdletSeverity::Critical)
        );

        let low = CmdletDatabase::get_by_severity(CmdletSeverity::Low);
        assert!(!low.is_empty());
        assert!(low.iter().all(|c| c.severity == CmdletSeverity::Low));
    }

    #[test]
    fn test_get_by_category() {
        let code_exec = CmdletDatabase::get_by_category(CmdletCategory::CodeExecution);
        assert!(!code_exec.is_empty());
        assert!(
            code_exec
                .iter()
                .all(|c| c.category == CmdletCategory::CodeExecution)
        );
    }

    #[test]
    fn test_count_by_severity() {
        let counts = CmdletDatabase::count_by_severity();
        assert!(counts.get(&CmdletSeverity::Critical).copied().unwrap_or(0) > 0);
        assert!(counts.get(&CmdletSeverity::High).copied().unwrap_or(0) > 0);
    }

    #[test]
    fn test_cmdlet_info_display() {
        let info = CmdletDatabase::get_info("invoke-expression").unwrap();
        assert_eq!(info.severity.to_string(), "CRITICAL");
        assert_eq!(info.category.to_string(), "Code Execution");
    }

    #[test]
    fn test_dangerous_patterns_coverage() {
        let info = CmdletDatabase::get_info("invoke-expression").unwrap();
        assert!(!info.dangerous_patterns.is_empty());
        assert!(info.dangerous_patterns.contains(&"iex"));
    }
}
