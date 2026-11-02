use std::path::PathBuf;

use anyhow::Context;
use vtcode_agent_plugins::{FileSystemPluginLoader, PluginLoader};

pub(super) async fn handle_list() -> anyhow::Result<()> {
    println!("Installed Agent Plugins:");
    println!();
    let roots = super::plugin_roots();

    // `read_dir` and `loader.load` are blocking filesystem operations; run
    // them off the async executor and collect printable rows. See `# Blocking`
    // docs in `src/agent/runloop/git.rs`.
    let rows = tokio::task::spawn_blocking(move || collect_plugin_rows(roots))
        .await
        .context("plugin list task panicked")??;

    for row in rows {
        match row {
            PluginRow::Valid { name, version, skill_count, mcp_count, path } => {
                println!(
                    "  {name} (v{version}) — {skill_count} skill(s), {mcp_count} MCP server(s) — {}",
                    path.display()
                );
            }
            PluginRow::Invalid { path, error } => {
                vtcode_commons::diagnostics::stderr(format_args!("  {} — invalid: {error}", path.display()));
            }
        }
    }
    Ok(())
}

enum PluginRow {
    Valid {
        name: String,
        version: String,
        skill_count: usize,
        mcp_count: usize,
        path: PathBuf,
    },
    Invalid {
        path: PathBuf,
        error: String,
    },
}

fn collect_plugin_rows(roots: Vec<PathBuf>) -> std::io::Result<Vec<PluginRow>> {
    let loader = FileSystemPluginLoader::new();
    let mut rows = Vec::new();
    for root in roots {
        if !root.is_dir() {
            continue;
        }
        for entry in std::fs::read_dir(&root)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let plugin_json = path.join("plugin.json");
            if !plugin_json.is_file() {
                continue;
            }
            match loader.load(&path) {
                Ok(loaded) => {
                    let skill_count = loaded.skills.len();
                    let mcp_count = loaded.mcp.as_ref().map(|m| m.servers.len()).unwrap_or(0);
                    rows.push(PluginRow::Valid {
                        name: loaded.manifest.name,
                        version: loaded.manifest.version.unwrap_or_else(|| "unknown".to_string()),
                        skill_count,
                        mcp_count,
                        path,
                    });
                }
                Err(e) => rows.push(PluginRow::Invalid { path, error: e.to_string() }),
            }
        }
    }
    Ok(rows)
}
