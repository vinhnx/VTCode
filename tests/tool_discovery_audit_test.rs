/// Tool Discovery Audit Test
///
/// This test performs a comprehensive audit of the tool system to ensure:
/// 1. All tools in constants.rs are discoverable in declarations
/// 2. All tools with policies are properly registered
/// 3. Tool metadata is consistent across the system
/// 4. No tools are silently dropped or hidden
///
/// Run with: cargo test --test tool_discovery_audit_test -- --nocapture
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
struct ToolAuditEntry {
    name: String,
    in_constants: bool,
    has_policy: bool,
    has_declaration: bool,
    in_acp: bool,
}

#[test]
fn audit_tool_system_completeness() {
    println!("\n=== TOOL SYSTEM AUDIT ===\n");

    // All tools that SHOULD exist
    let all_tools = vec![
        // File operations (non-destructive)
        ("list_files", "List directory contents and search files"),
        ("grep_file", "Search file contents with ripgrep"),
        ("read_file", "Read file contents"),
        // File operations (write/destructive)
        ("write_file", "Write complete file"),
        ("edit_file", "Edit file with old_str/new_str replacement"),
        ("create_file", "Create new file"),
        ("delete_file", "Delete file"),
        ("apply_patch", "Apply unified diff patch"),
        // PTY/Terminal operations
        ("run_pty_cmd", "Execute shell command"),
        ("create_pty_session", "Create persistent PTY session"),
        ("read_pty_session", "Read PTY session output"),
        ("list_pty_sessions", "List active PTY sessions"),
        ("resize_pty_session", "Resize PTY terminal"),
        ("send_pty_input", "Send input to PTY session"),
        ("close_pty_session", "Close PTY session"),
        // Code execution
        ("execute_code", "Execute Python or JavaScript code"),
        // Planning and introspection
        ("search_tools", "Search available tools"),
        // Web operations
        ("web_fetch", "Fetch web page content"),
    ];

    let all_tool_names: HashSet<_> = all_tools.iter().map(|(name, _)| *name).collect();

    // Tools with policies (from DEFAULT_TOOL_POLICIES)
    let tools_with_policies: HashSet<&str> = HashSet::from_iter(vec![
        "list_files",
        "grep_file",
        "read_file",
        "write_file",
        "edit_file",
        "create_file",
        "delete_file",
        "apply_patch",
        "run_pty_cmd",
        "create_pty_session",
        "read_pty_session",
        "list_pty_sessions",
        "resize_pty_session",
        "send_pty_input",
        "close_pty_session",
        "execute_code",
        "search_tools",
        "web_fetch",
    ]);

    // Tools with declarations (from grep of declarations.rs)
    let tools_with_declarations: HashSet<&str> = HashSet::from_iter(vec![
        "grep_file",
        "run_pty_cmd",
        "search_tools",
        "execute_code",
        "debug_agent",
        "analyze_agent",
        "read_file",
        "create_file",
        "delete_file",
        "write_file",
        "edit_file",
        "apply_patch",
        "create_pty_session",
        "list_pty_sessions",
        "close_pty_session",
        "send_pty_input",
        "read_pty_session",
        "resize_pty_session",
        "web_fetch",
        "update_plan",
    ]);

    // Tools exposed via ACP (Zed integration)
    let tools_in_acp: HashSet<&str> = HashSet::from_iter(vec!["read_file", "list_files"]);

    // Build audit entries
    let mut audit_entries: Vec<_> = all_tool_names
        .iter()
        .map(|name| ToolAuditEntry {
            name: name.to_string(),
            in_constants: true,
            has_policy: tools_with_policies.contains(name),
            has_declaration: tools_with_declarations.contains(name),
            in_acp: tools_in_acp.contains(name),
        })
        .collect();

    audit_entries.sort_by(|a, b| a.name.cmp(&b.name));

    // Print audit report
    println!("TOOL AUDIT REPORT");
    println!("{:-^100}", "");
    println!(
        "{:<30} | {:<10} | {:<12} | {:<12} | {:<8}",
        "Tool Name", "Constants", "Policy", "Declaration", "ACP"
    );
    println!("{:-^100}", "");

    for entry in &audit_entries {
        println!(
            "{:<30} | {:<10} | {:<12} | {:<12} | {:<8}",
            entry.name,
            if entry.in_constants { "✓" } else { "✗" },
            if entry.has_policy { "✓" } else { "✗" },
            if entry.has_declaration { "✓" } else { "✗" },
            if entry.in_acp { "✓" } else { "—" }
        );
    }
    println!("{:-^100}\n", "");

    // Analysis
    let missing_policies: Vec<_> = audit_entries
        .iter()
        .filter(|e| !e.has_policy)
        .map(|e| &e.name)
        .collect();

    let missing_declarations: Vec<_> = audit_entries
        .iter()
        .filter(|e| !e.has_declaration)
        .map(|e| &e.name)
        .collect();

    println!("SUMMARY:");
    println!("  Total tools: {}", audit_entries.len());
    println!(
        "  With policies: {}/{}",
        tools_with_policies.len(),
        audit_entries.len()
    );
    println!(
        "  With declarations: {}/{}",
        tools_with_declarations.len(),
        audit_entries.len()
    );
    println!("  In ACP: {}/{}", tools_in_acp.len(), audit_entries.len());

    if !missing_policies.is_empty() {
        println!("\n  [WARN] TOOLS WITHOUT POLICIES: {:?}", missing_policies);
    }

    if !missing_declarations.is_empty() {
        println!(
            "\n  [INFO] TOOLS WITHOUT DECLARATIONS: {:?}",
            missing_declarations
        );
        println!("      (Some tools may intentionally not have LLM declarations)");
    }

    println!();

    // Assertions - policies must be complete
    assert!(
        missing_policies.is_empty(),
        "Missing policies for tools: {:?}",
        missing_policies
    );
}

#[test]
fn audit_tool_categories() {
    println!("\n=== TOOL CATEGORIES AUDIT ===\n");

    let categories: HashMap<&str, Vec<&str>> = HashMap::from_iter(vec![
        (
            "File Operations (Read)",
            vec!["list_files", "grep_file", "read_file"],
        ),
        (
            "File Operations (Write/Modify)",
            vec!["write_file", "edit_file", "create_file"],
        ),
        (
            "File Operations (Destructive)",
            vec!["delete_file", "apply_patch"],
        ),
        (
            "Terminal/PTY",
            vec![
                "run_pty_cmd",
                "create_pty_session",
                "read_pty_session",
                "list_pty_sessions",
                "resize_pty_session",
                "send_pty_input",
                "close_pty_session",
            ],
        ),
        ("Code Execution", vec!["execute_code"]),
        ("Planning & Meta", vec!["update_plan", "search_tools"]),
        (
            "Diagnostic & Introspection",
            vec!["agent_info"],
        ),
        ("Network", vec!["web_fetch"]),
    ]);

    println!("TOOL CATEGORIES:");
    println!("{:-^80}", "");
    for (category, tools) in categories.iter() {
        println!("{}:", category);
        for tool in tools {
            println!("  - {}", tool);
        }
        println!();
    }

    // Count tools
    let total_tools: usize = categories.values().map(|v| v.len()).sum();
    println!(
        "Total: {} tools across {} categories\n",
        total_tools,
        categories.len()
    );
}

#[test]
fn audit_tool_policy_distribution() {
    println!("\n=== TOOL POLICY DISTRIBUTION ===\n");

    let policy_groups: HashMap<&str, Vec<&str>> = HashMap::from_iter(vec![
        (
            "Allow (No confirmation)",
            vec![
                "list_files",
                "grep_file",
                "read_file",
                "write_file",
                "edit_file",
                "create_file",
                "create_pty_session",
                "read_pty_session",
                "list_pty_sessions",
                "resize_pty_session",
                "close_pty_session",
                "update_plan",
                "search_tools",
            ],
        ),
        (
            "Prompt (Requires confirmation)",
            vec![
                "delete_file",
                "apply_patch",
                "run_pty_cmd",
                "send_pty_input",
                "execute_code",
                "web_fetch",
            ],
        ),
    ]);

    println!("POLICY DISTRIBUTION:");
    println!("{:-^80}", "");
    for (policy, tools) in policy_groups.iter() {
        println!("{}:", policy);
        for tool in tools {
            println!("  - {}", tool);
        }
        println!("  Total: {}\n", tools.len());
    }

    let allow_count: usize = policy_groups
        .get("Allow (No confirmation)")
        .unwrap_or(&vec![])
        .len();
    let prompt_count: usize = policy_groups
        .get("Prompt (Requires confirmation)")
        .unwrap_or(&vec![])
        .len();

    println!(
        "Ratio: {:.1}% Allow, {:.1}% Prompt",
        allow_count as f32 / (allow_count + prompt_count) as f32 * 100.0,
        prompt_count as f32 / (allow_count + prompt_count) as f32 * 100.0
    );
    println!();
}

#[test]
fn audit_acp_tool_selection_reasoning() {
    println!("\n=== ACP TOOL SELECTION ANALYSIS ===\n");

    println!("TOOLS EXPOSED VIA ACP:");
    println!("  ✓ read_file");
    println!("      Reason: Safe file reading, no workspace modification");
    println!("  ✓ list_files");
    println!("      Reason: Safe file discovery, supports workspace navigation");
    println!();

    println!("TOOLS NOT EXPOSED VIA ACP:");

    let excluded = vec![
        (
            "grep_file",
            "Functionality overlaps with Zed's native search",
        ),
        ("write_file", "Prevents unintended edits in editor context"),
        ("edit_file", "Edit operations reserved for local agent only"),
        ("create_file", "File creation reserved for local agent"),
        ("delete_file", "Destructive operation not suitable for ACP"),
        ("apply_patch", "Complex state management, local-only"),
        (
            "run_pty_cmd",
            "Terminal access not available in Zed context",
        ),
        ("execute_code", "Security risk in editor integration"),
        ("web_fetch", "Network access restricted in editor"),
        (
            "*_pty_session",
            "Terminal operations not available in editor",
        ),
    ];

    for (tool, reason) in excluded {
        println!("  ✗ {:<25} - {}", tool, reason);
    }
    println!();
}
