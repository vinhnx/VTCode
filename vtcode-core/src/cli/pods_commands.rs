//! CLI commands for GPU pod management.

use crate::cli::PodsCommands;
use crate::pods::{
    PodGpu, PodManager, PodStartRequest, PodState, PodStatusDetail, PodStatusReport,
};
use crate::utils::colors::{bold, cyan, green, underline, yellow};
use anyhow::{Result, anyhow};

/// Handle GPU pod commands.
pub async fn handle_pods_command(command: PodsCommands) -> Result<()> {
    let manager = PodManager::new()?;

    match command {
        PodsCommands::Start {
            name,
            model,
            pod_name,
            ssh,
            gpus,
            models_path,
            profile,
            gpus_count,
            memory,
            context,
        } => {
            let request = PodStartRequest {
                pod_name,
                ssh,
                gpus: parse_gpu_entries(&gpus)?,
                models_path,
                name,
                model,
                profile,
                requested_gpu_count: gpus_count,
                memory,
                context,
            };
            let result = manager.start_model(request).await?;
            print_start_result(&result);
        }
        PodsCommands::Stop { name } => {
            let stopped = manager
                .stop_model(&name)
                .await?
                .ok_or_else(|| anyhow!("unknown model '{}'", name))?;
            println!(
                "{} Stopped {} (pid {})",
                green("✓"),
                cyan(&name),
                stopped.pid
            );
        }
        PodsCommands::StopAll => {
            let count = manager.stop_all_models().await?;
            println!("{} Stopped {} model(s)", green("✓"), count);
        }
        PodsCommands::List => {
            let report = manager.list_models().await?;
            print_list_report(&report);
        }
        PodsCommands::Logs { name } => {
            manager.stream_logs(&name).await?;
        }
        PodsCommands::KnownModels => {
            let report = manager.known_models().await?;
            print_known_models(&report, manager.load_state().await?.active_pod.as_ref());
        }
    }

    Ok(())
}

fn parse_gpu_entries(raw: &[String]) -> Result<Vec<PodGpu>> {
    raw.iter()
        .map(|entry| {
            let (id, name) = entry
                .split_once(':')
                .or_else(|| entry.split_once('='))
                .ok_or_else(|| anyhow!("invalid GPU entry '{}'; expected ID:NAME", entry))?;
            let id = id
                .trim()
                .parse::<u32>()
                .map_err(|_| anyhow!("invalid GPU id '{}'", id.trim()))?;
            let name = name.trim();
            if name.is_empty() {
                return Err(anyhow!("GPU name cannot be empty"));
            }
            Ok(PodGpu {
                id,
                name: name.to_string(),
            })
        })
        .collect()
}

fn print_start_result(result: &crate::pods::PodStartResult) {
    println!("{} Started {}", green("✓"), bold(&result.entry.model));
    println!("  Pod: {}", cyan(&result.pod.name));
    println!("  Profile: {}", cyan(&result.entry.profile));
    println!("  PID: {}", cyan(&result.entry.pid.to_string()));
    println!("  Port: {}", cyan(&result.entry.port.to_string()));
    println!(
        "  GPUs: {}",
        cyan(
            &result
                .entry
                .gpu_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        )
    );
    println!("  Launch: {}", yellow(&result.launch_command));
}

fn print_list_report(report: &PodStatusReport) {
    println!("{}", underline(&bold("Active Pod")));
    println!("Pod: {}", cyan(&report.pod_name));
    println!();

    if report.entries.is_empty() {
        println!("{}", yellow("No running models"));
        return;
    }

    for entry in &report.entries {
        println!(
            "{} {} | model={} | port={} | pid={} | gpus={}",
            status_symbol(entry.status),
            bold(&entry.name),
            entry.model,
            entry.port,
            entry.pid,
            entry
                .gpu_ids
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

fn print_known_models(report: &crate::pods::KnownModelsReport, active_pod: Option<&PodState>) {
    println!("{}", underline(&bold("Known Models")));
    if let Some(pod) = active_pod {
        println!("Pod: {}", cyan(&pod.name));
        println!("GPUs: {}", cyan(&pod.gpu_count().to_string()));
    }
    println!();

    println!("{}", green("Compatible"));
    for model in &report.compatible {
        print_model_detail(model, green("  ✓"));
    }

    println!();
    println!("{}", yellow("Incompatible"));
    for model in &report.incompatible {
        print_model_detail(model, yellow("  •"));
    }
}

fn print_model_detail(model: &PodStatusDetail, prefix: impl std::fmt::Display) {
    println!(
        "{} {} ({}, {} GPU{})",
        prefix,
        cyan(&model.name),
        model.model,
        model.gpu_count,
        if model.gpu_count == 1 { "" } else { "s" }
    );
}

fn status_symbol(status: crate::pods::PodHealth) -> &'static str {
    match status {
        crate::pods::PodHealth::Running => "✓",
        crate::pods::PodHealth::Starting => "…",
        crate::pods::PodHealth::Crashed => "✗",
        crate::pods::PodHealth::Dead => "•",
    }
}
