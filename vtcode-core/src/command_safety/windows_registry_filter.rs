//! Windows Registry Access Path Filter (Phase 6.3)
//!
//! Analyzes PowerShell registry access patterns and filters dangerous paths.
//! Provides context-aware filtering for:
//! - Registry hive access (HKLM, HKCU, HKU, HKCR, HKCC)
//! - Dangerous registry paths (Run, Services, Drivers, etc.)
//! - Access methods (Get-Item, Set-Item, New-ItemProperty, etc.)
//! - Privilege levels required

use std::collections::HashMap;

/// Registry access risk level
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RegistryRiskLevel {
    Safe = 0,
    Low = 1,
    Medium = 2,
    High = 3,
    Critical = 4,
}

impl std::fmt::Display for RegistryRiskLevel {
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

/// Registry path metadata
#[derive(Debug, Clone)]
pub struct RegistryPathInfo {
    pub path_pattern: &'static str,
    pub risk_level: RegistryRiskLevel,
    pub hives: &'static [&'static str],
    pub description: &'static str,
}

/// Get dangerous registry paths
pub fn get_dangerous_registry_paths() -> HashMap<&'static str, RegistryPathInfo> {
    let mut m = HashMap::new();

    // ──── CRITICAL: System Integrity & Privilege Escalation ────
    m.insert(
        "run",
        RegistryPathInfo {
            path_pattern: "Run",
            risk_level: RegistryRiskLevel::Critical,
            hives: &["HKLM", "HKCU"],
            description: "Auto-run programs on system startup",
        },
    );

    m.insert(
        "runonce",
        RegistryPathInfo {
            path_pattern: "RunOnce",
            risk_level: RegistryRiskLevel::Critical,
            hives: &["HKLM", "HKCU"],
            description: "Auto-run once on next login",
        },
    );

    m.insert(
        "services",
        RegistryPathInfo {
            path_pattern: "Services",
            risk_level: RegistryRiskLevel::Critical,
            hives: &["HKLM"],
            description: "Windows services configuration",
        },
    );

    m.insert(
        "drivers",
        RegistryPathInfo {
            path_pattern: "Drivers",
            risk_level: RegistryRiskLevel::Critical,
            hives: &["HKLM"],
            description: "Kernel drivers installation",
        },
    );

    m.insert(
        "sam",
        RegistryPathInfo {
            path_pattern: "SAM",
            risk_level: RegistryRiskLevel::Critical,
            hives: &["HKLM"],
            description: "Security Account Manager (password hashes)",
        },
    );

    m.insert(
        "security",
        RegistryPathInfo {
            path_pattern: "Security",
            risk_level: RegistryRiskLevel::Critical,
            hives: &["HKLM"],
            description: "Security policy and audit settings",
        },
    );

    // ──── HIGH: Privilege & Credential Access ────
    m.insert(
        "credentialmanager",
        RegistryPathInfo {
            path_pattern: "Credential Manager",
            risk_level: RegistryRiskLevel::High,
            hives: &["HKCU"],
            description: "Stored credentials and vault access",
        },
    );

    m.insert(
        "lsa",
        RegistryPathInfo {
            path_pattern: "LSA",
            risk_level: RegistryRiskLevel::High,
            hives: &["HKLM"],
            description: "Local Security Authority secrets",
        },
    );

    m.insert(
        "authentication",
        RegistryPathInfo {
            path_pattern: "Authentication",
            risk_level: RegistryRiskLevel::High,
            hives: &["HKLM"],
            description: "Authentication providers and methods",
        },
    );

    m.insert(
        "kerberos",
        RegistryPathInfo {
            path_pattern: "Kerberos",
            risk_level: RegistryRiskLevel::High,
            hives: &["HKLM", "HKCU"],
            description: "Kerberos authentication settings",
        },
    );

    // ──── HIGH: Windows Defender & Security ────
    m.insert(
        "defender",
        RegistryPathInfo {
            path_pattern: "Defender",
            risk_level: RegistryRiskLevel::High,
            hives: &["HKLM"],
            description: "Windows Defender/Antimalware settings",
        },
    );

    m.insert(
        "windows defender",
        RegistryPathInfo {
            path_pattern: "Windows Defender",
            risk_level: RegistryRiskLevel::High,
            hives: &["HKLM"],
            description: "Windows Defender configuration",
        },
    );

    m.insert(
        "policies",
        RegistryPathInfo {
            path_pattern: "Policies",
            risk_level: RegistryRiskLevel::High,
            hives: &["HKLM", "HKCU"],
            description: "Group Policy Objects and security policies",
        },
    );

    m.insert(
        "uac",
        RegistryPathInfo {
            path_pattern: "UAC",
            risk_level: RegistryRiskLevel::High,
            hives: &["HKLM"],
            description: "User Account Control bypass settings",
        },
    );

    // ──── HIGH: Code Execution & AppInit ────
    m.insert(
        "appinit_dlls",
        RegistryPathInfo {
            path_pattern: "AppInit_DLLs",
            risk_level: RegistryRiskLevel::High,
            hives: &["HKLM"],
            description: "DLLs loaded into every process",
        },
    );

    m.insert(
        "winlogon",
        RegistryPathInfo {
            path_pattern: "Winlogon",
            risk_level: RegistryRiskLevel::High,
            hives: &["HKLM", "HKCU"],
            description: "Logon process and shell configuration",
        },
    );

    m.insert(
        "shellexecutehooks",
        RegistryPathInfo {
            path_pattern: "ShellExecuteHooks",
            risk_level: RegistryRiskLevel::High,
            hives: &["HKLM", "HKCU"],
            description: "Shell execution hooks for code injection",
        },
    );

    // ──── MEDIUM: Configuration & Persistence ────
    m.insert(
        "environment",
        RegistryPathInfo {
            path_pattern: "Environment",
            risk_level: RegistryRiskLevel::Medium,
            hives: &["HKLM", "HKCU"],
            description: "Environment variables (can affect execution)",
        },
    );

    m.insert(
        "proxy",
        RegistryPathInfo {
            path_pattern: "Proxy",
            risk_level: RegistryRiskLevel::Medium,
            hives: &["HKLM", "HKCU"],
            description: "Proxy settings for network access",
        },
    );

    m.insert(
        "installer",
        RegistryPathInfo {
            path_pattern: "Installer",
            risk_level: RegistryRiskLevel::Medium,
            hives: &["HKLM"],
            description: "Windows Installer configuration",
        },
    );

    m.insert(
        "network",
        RegistryPathInfo {
            path_pattern: "Network",
            risk_level: RegistryRiskLevel::Medium,
            hives: &["HKLM", "HKCU"],
            description: "Network configuration and protocols",
        },
    );

    // ──── LOW: Information Gathering ────
    m.insert(
        "currentversion",
        RegistryPathInfo {
            path_pattern: "CurrentVersion",
            risk_level: RegistryRiskLevel::Low,
            hives: &["HKLM"],
            description: "Windows version and product information",
        },
    );

    m.insert(
        "uninstall",
        RegistryPathInfo {
            path_pattern: "Uninstall",
            risk_level: RegistryRiskLevel::Low,
            hives: &["HKLM", "HKCU"],
            description: "Installed programs list",
        },
    );

    m
}

/// Registry access pattern
#[derive(Debug, Clone)]
pub struct RegistryAccessPattern {
    pub hive: String,
    pub path: String,
    pub access_method: String,
    pub risk_level: RegistryRiskLevel,
    pub is_write_operation: bool,
}

/// Registry access analyzer
pub struct RegistryAccessFilter;

impl RegistryAccessFilter {
    /// Get registry path metadata
    pub fn get_path_info(path_key: &str) -> Option<RegistryPathInfo> {
        let paths = get_dangerous_registry_paths();
        let normalized = path_key.to_lowercase();

        // Exact match
        if let Some(info) = paths.get(&normalized.as_str()) {
            return Some(info.clone());
        }

        // Pattern match (path contains key)
        for (_, info) in paths.iter() {
            if normalized.contains(&info.path_pattern.to_lowercase()) {
                return Some(info.clone());
            }
        }

        None
    }

    /// Analyze registry access in PowerShell script
    pub fn analyze_registry_access(script: &str) -> Vec<RegistryAccessPattern> {
        let mut patterns = Vec::new();
        let script_lower = script.to_lowercase();

        // Detect Get-Item/Get-ItemProperty (read operations)
        if script_lower.contains("get-item") || script_lower.contains("get-itemproperty") {
            for (key, info) in get_dangerous_registry_paths().iter() {
                if script_lower.contains(&format!("hklm:\\{}", info.path_pattern.to_lowercase()))
                    || script_lower
                        .contains(&format!("hkcu:\\{}", info.path_pattern.to_lowercase()))
                {
                    let hive = if script_lower
                        .contains(&format!("hklm:\\{}", info.path_pattern.to_lowercase()))
                    {
                        "HKLM".to_string()
                    } else {
                        "HKCU".to_string()
                    };

                    patterns.push(RegistryAccessPattern {
                        hive,
                        path: info.path_pattern.to_string(),
                        access_method: "Get-Item/Get-ItemProperty".to_string(),
                        risk_level: info.risk_level,
                        is_write_operation: false,
                    });
                }
            }
        }

        // Detect Set-Item/Set-ItemProperty (write operations)
        if script_lower.contains("set-item") || script_lower.contains("set-itemproperty") {
            for (_, info) in get_dangerous_registry_paths().iter() {
                let hive_patterns = vec!["hklm:\\", "hkcu:\\"];
                for hive_pattern in hive_patterns {
                    let full_pattern =
                        format!("{}{}", hive_pattern, info.path_pattern.to_lowercase());
                    if script_lower.contains(&full_pattern) {
                        let hive = hive_pattern.replace(":\\", "").to_uppercase();
                        patterns.push(RegistryAccessPattern {
                            hive,
                            path: info.path_pattern.to_string(),
                            access_method: "Set-Item/Set-ItemProperty".to_string(),
                            risk_level: info.risk_level,
                            is_write_operation: true,
                        });
                    }
                }
            }
        }

        // Detect New-ItemProperty (create operations)
        if script_lower.contains("new-itemproperty") {
            for (_, info) in get_dangerous_registry_paths().iter() {
                let pattern = format!("run|runonce|services", info.path_pattern.to_lowercase());
                if script_lower.contains(&pattern) {
                    patterns.push(RegistryAccessPattern {
                        hive: "HKLM/HKCU".to_string(),
                        path: info.path_pattern.to_string(),
                        access_method: "New-ItemProperty".to_string(),
                        risk_level: info.risk_level,
                        is_write_operation: true,
                    });
                }
            }
        }

        patterns
    }

    /// Check if registry access is dangerous
    pub fn is_dangerous_registry_access(script: &str) -> bool {
        let patterns = Self::analyze_registry_access(script);

        patterns.iter().any(|p| {
            // Critical or High risk with write operations is dangerous
            (p.is_write_operation && p.risk_level >= RegistryRiskLevel::High)
                || p.risk_level == RegistryRiskLevel::Critical
        })
    }

    /// Get maximum registry risk level in script
    pub fn get_max_registry_risk(script: &str) -> RegistryRiskLevel {
        Self::analyze_registry_access(script)
            .iter()
            .map(|p| p.risk_level)
            .max()
            .unwrap_or(RegistryRiskLevel::Safe)
    }

    /// Filter registry paths based on maximum allowed risk
    pub fn filter_by_risk_level(
        script: &str,
        max_risk: RegistryRiskLevel,
    ) -> Vec<RegistryAccessPattern> {
        Self::analyze_registry_access(script)
            .into_iter()
            .filter(|p| p.risk_level <= max_risk)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_run_path_info() {
        let info = RegistryAccessFilter::get_path_info("run").unwrap();
        assert_eq!(info.risk_level, RegistryRiskLevel::Critical);
        assert!(info.hives.contains(&"HKLM"));
    }

    #[test]
    fn test_case_insensitive_lookup() {
        assert!(RegistryAccessFilter::get_path_info("RUN").is_some());
        assert!(RegistryAccessFilter::get_path_info("Run").is_some());
    }

    #[test]
    fn test_analyze_get_item_access() {
        let script = "Get-Item -Path HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
        let patterns = RegistryAccessFilter::analyze_registry_access(script);
        assert!(!patterns.is_empty());
        assert!(!patterns[0].is_write_operation);
    }

    #[test]
    fn test_analyze_set_item_access() {
        let script = "Set-Item -Path HKLM:\\Software\\Microsoft\\Windows\\Run -Value malware.exe";
        let patterns = RegistryAccessFilter::analyze_registry_access(script);
        assert!(!patterns.is_empty());
        assert!(patterns[0].is_write_operation);
    }

    #[test]
    fn test_is_dangerous_registry_access() {
        let dangerous =
            "Set-ItemProperty -Path HKLM:\\System\\CurrentControlSet\\Services\\* -Value *";
        assert!(RegistryAccessFilter::is_dangerous_registry_access(
            dangerous
        ));

        let safe = "Get-Item -Path HKCU:\\Software";
        assert!(!RegistryAccessFilter::is_dangerous_registry_access(safe));
    }

    #[test]
    fn test_get_max_registry_risk() {
        let script = "Get-Item HKLM:\\SAM; Set-Item HKCU:\\Run";
        let max_risk = RegistryAccessFilter::get_max_registry_risk(script);
        assert!(max_risk >= RegistryRiskLevel::Critical);
    }

    #[test]
    fn test_filter_by_risk_level() {
        let script = r#"
            Get-Item HKLM:\\Run
            Get-Item HKCU:\\Environment
        "#;
        let low_risk = RegistryAccessFilter::filter_by_risk_level(script, RegistryRiskLevel::Low);
        assert!(low_risk.is_empty());

        let medium_risk =
            RegistryAccessFilter::filter_by_risk_level(script, RegistryRiskLevel::Medium);
        assert!(!medium_risk.is_empty());
    }

    #[test]
    fn test_registry_risk_level_ordering() {
        assert!(RegistryRiskLevel::Critical > RegistryRiskLevel::High);
        assert!(RegistryRiskLevel::High > RegistryRiskLevel::Medium);
        assert!(RegistryRiskLevel::Medium > RegistryRiskLevel::Low);
        assert!(RegistryRiskLevel::Low > RegistryRiskLevel::Safe);
    }
}
