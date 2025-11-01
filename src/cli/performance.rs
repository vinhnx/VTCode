use anyhow::Result;
use sysinfo::System;
use vtcode_core::utils::colors::style;

/// Handle the performance command
pub async fn handle_performance_command() -> Result<()> {
    println!("{}", style("Performance Metrics").blue().bold());

    let mut sys = System::new_all();
    sys.refresh_all();

    println!("\nSystem:");
    println!("  OS: {}", std::env::consts::OS);
    println!("  Arch: {}", std::env::consts::ARCH);
    println!("  Family: {}", std::env::consts::FAMILY);

    println!("\nMemory:");
    println!(
        "  Total: {:.2} GB",
        sys.total_memory() as f64 / 1024.0 / 1024.0 / 1024.0
    );
    println!(
        "  Used: {:.2} GB",
        sys.used_memory() as f64 / 1024.0 / 1024.0 / 1024.0
    );
    println!(
        "  Available: {:.2} GB",
        sys.available_memory() as f64 / 1024.0 / 1024.0 / 1024.0
    );

    println!("\nCPU:");
    println!("  Cores: {}", sys.cpus().len());
    println!("  Usage: {:.1}%", sys.global_cpu_usage());

    println!("\nProcesses: {}", sys.processes().len());
    if let Some(process) = sys.process(sysinfo::Pid::from_u32(std::process::id())) {
        println!(
            "  Self RSS: {:.2} MB",
            process.memory() as f64 / 1024.0 / 1024.0
        );
        println!("  Self CPU: {:.1}%", process.cpu_usage());
    }

    Ok(())
}
