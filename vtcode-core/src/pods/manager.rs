use crate::pods::catalog::{PodCatalog, PodProfile};
use crate::pods::state::{PodGpu, PodHealth, PodState, PodsState, RunningModel};
use crate::pods::store::PodsStore;
use crate::pods::transport::{PodTransport, SshTransport};
use anyhow::{Context, Result, anyhow};
use parking_lot::RwLock;
use std::collections::BTreeMap;
use std::sync::Arc;

const DEFAULT_START_PORT: u16 = 8001;
const DEFAULT_LOG_DIR: &str = ".vllm_logs";

/// Request payload for starting a model on a pod.
#[derive(Debug, Clone)]
pub struct PodStartRequest {
    pub pod_name: Option<String>,
    pub ssh: Option<String>,
    pub gpus: Vec<PodGpu>,
    pub models_path: Option<String>,
    pub name: String,
    pub model: String,
    pub profile: Option<String>,
    pub requested_gpu_count: Option<usize>,
    pub memory: Option<f32>,
    pub context: Option<String>,
}

/// Result of a successful model launch.
#[derive(Debug, Clone)]
pub struct PodStartResult {
    pub pod: PodState,
    pub entry: RunningModel,
    pub profile: PodProfile,
    pub launch_command: String,
}

/// Row returned by `pods list`.
#[derive(Debug, Clone)]
pub struct PodListEntry {
    pub name: String,
    pub model: String,
    pub port: u16,
    pub pid: u32,
    pub gpu_ids: Vec<u32>,
    pub status: PodHealth,
}

/// Row returned by `pods known-models`.
#[derive(Debug, Clone)]
pub struct PodStatusDetail {
    pub name: String,
    pub model: String,
    pub gpu_count: usize,
}

/// Split known models into compatible and incompatible groups.
#[derive(Debug, Clone)]
pub struct KnownModelsReport {
    pub compatible: Vec<PodStatusDetail>,
    pub incompatible: Vec<PodStatusDetail>,
}

/// `pods list` report.
#[derive(Debug, Clone)]
pub struct PodStatusReport {
    pub pod_name: String,
    pub entries: Vec<PodListEntry>,
}

/// Pod manager coordinating persisted state, catalog lookup, and SSH execution.
#[derive(Clone)]
pub struct PodManager {
    store: PodsStore,
    transport: Arc<dyn PodTransport>,
    cached_state: Arc<RwLock<Option<PodsState>>>,
    cached_catalog: Arc<RwLock<Option<PodCatalog>>>,
}

impl PodManager {
    pub fn new() -> Result<Self> {
        Ok(Self::with_transport(
            PodsStore::default_store()?,
            Arc::new(SshTransport),
        ))
    }

    pub fn with_transport(store: PodsStore, transport: Arc<dyn PodTransport>) -> Self {
        Self {
            store,
            transport,
            cached_state: Arc::new(RwLock::new(None)),
            cached_catalog: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn load_state(&self) -> Result<PodsState> {
        if let Some(state) = self.cached_state.read().clone() {
            return Ok(state);
        }

        let state = self.store.load_state().await?;
        *self.cached_state.write() = Some(state.clone());
        Ok(state)
    }

    pub async fn load_catalog(&self) -> Result<PodCatalog> {
        if let Some(catalog) = self.cached_catalog.read().clone() {
            return Ok(catalog);
        }

        let catalog = self.store.load_catalog().await?;
        *self.cached_catalog.write() = Some(catalog.clone());
        Ok(catalog)
    }

    pub async fn start_model(&self, request: PodStartRequest) -> Result<PodStartResult> {
        let mut state = self.load_state().await?;
        let catalog = self.load_catalog().await?;
        let pod = self.resolve_active_pod(&mut state, &request).await?;
        let profile = self.resolve_profile(&catalog, &pod, &request)?;
        let gpu_count = request
            .requested_gpu_count
            .unwrap_or(profile.gpu_count)
            .max(1);

        if gpu_count > pod.gpu_count() {
            return Err(anyhow!(
                "requested {} GPUs but pod '{}' only has {}",
                gpu_count,
                pod.name,
                pod.gpu_count()
            ));
        }

        let selected_gpu_ids = select_gpus(&pod, gpu_count);
        let port = next_port(&pod);
        let sanitized_name = sanitize_component(&request.name);
        let run_path = format!("/tmp/model_run_{sanitized_name}.sh");
        let wrapper_path = format!("/tmp/model_wrapper_{sanitized_name}.sh");
        let log_path = format!("~/{DEFAULT_LOG_DIR}/{sanitized_name}.log");
        let vllm_args = render_args(
            &profile.vllm_args,
            request.memory,
            request.context.as_deref(),
        )?;
        let run_script = render_run_script(
            &profile,
            &request.model,
            &request.name,
            port,
            &vllm_args,
            &selected_gpu_ids,
            pod.models_path.as_deref(),
        );
        let wrapper_script = render_wrapper_script(&run_path, &log_path);

        self.transport
            .write_file(&pod.ssh, &run_path, &run_script)
            .await?;
        self.transport
            .write_file(&pod.ssh, &wrapper_path, &wrapper_script)
            .await?;

        let chmod = self
            .transport
            .exec_capture(&pod.ssh, &format!("chmod +x {run_path} {wrapper_path}"))
            .await?;
        if !chmod.success {
            return Err(anyhow!("failed to chmod remote scripts: {}", chmod.stderr));
        }

        let launch_command = format!(
            "mkdir -p ~/{DEFAULT_LOG_DIR} && setsid {wrapper_path} >/dev/null 2>&1 < /dev/null & echo $!"
        );
        let launch = self
            .transport
            .exec_capture(&pod.ssh, &launch_command)
            .await?;
        if !launch.success {
            return Err(anyhow!("failed to launch remote model: {}", launch.stderr));
        }

        let pid = parse_pid(&launch.stdout)?;
        let entry = RunningModel {
            model: request.model.clone(),
            port,
            gpu_ids: selected_gpu_ids.clone(),
            pid,
            profile: profile.name.clone(),
        };

        let mut updated_pod = pod.clone();
        updated_pod
            .models
            .insert(request.name.clone(), entry.clone());
        state.active_pod = Some(updated_pod.clone());
        self.persist_state(&state).await?;

        Ok(PodStartResult {
            pod: updated_pod,
            entry,
            profile,
            launch_command,
        })
    }

    pub async fn stop_model(&self, name: &str) -> Result<Option<RunningModel>> {
        let mut state = self.load_state().await?;
        let Some(pod) = state.active_pod.as_mut() else {
            return Ok(None);
        };

        let Some(entry) = pod.models.remove(name) else {
            return Ok(None);
        };

        let command = format!(
            "pkill -TERM -P {} || true; kill {} || true",
            entry.pid, entry.pid
        );
        let output = self.transport.exec_capture(&pod.ssh, &command).await?;
        if !output.success {
            return Err(anyhow!(
                "failed to stop model '{}': {}",
                name,
                output.stderr
            ));
        }

        self.persist_state(&state).await?;
        Ok(Some(entry))
    }

    pub async fn stop_all_models(&self) -> Result<usize> {
        let mut state = self.load_state().await?;
        let Some(pod) = state.active_pod.as_mut() else {
            return Ok(0);
        };

        let pids = pod
            .models
            .values()
            .map(|entry| entry.pid.to_string())
            .collect::<Vec<_>>();

        if pids.is_empty() {
            return Ok(0);
        }

        let command = format!(
            "for PID in {}; do pkill -TERM -P \"$PID\" || true; kill \"$PID\" || true; done",
            pids.join(" ")
        );
        let output = self.transport.exec_capture(&pod.ssh, &command).await?;
        if !output.success {
            return Err(anyhow!("failed to stop models: {}", output.stderr));
        }

        let stopped = pod.models.len();
        pod.models.clear();
        self.persist_state(&state).await?;
        Ok(stopped)
    }

    pub async fn list_models(&self) -> Result<PodStatusReport> {
        let state = self.load_state().await?;
        let Some(pod) = state.active_pod.as_ref() else {
            return Err(anyhow!("no active pod configured"));
        };

        let mut entries = Vec::new();
        for (name, model) in &pod.models {
            let status = self.inspect_model(pod, name, model).await?;
            entries.push(PodListEntry {
                name: name.clone(),
                model: model.model.clone(),
                port: model.port,
                pid: model.pid,
                gpu_ids: model.gpu_ids.clone(),
                status,
            });
        }

        Ok(PodStatusReport {
            pod_name: pod.name.clone(),
            entries,
        })
    }

    pub async fn stream_logs(&self, name: &str) -> Result<()> {
        let state = self.load_state().await?;
        let Some(pod) = state.active_pod.as_ref() else {
            return Err(anyhow!("no active pod configured"));
        };
        let Some(entry) = pod.models.get(name) else {
            return Err(anyhow!("unknown model '{}'", name));
        };

        let log_path = format!("~/{DEFAULT_LOG_DIR}/{}.log", sanitize_component(name));
        let command = format!("tail -f {log_path}");
        let _ = entry;
        self.transport.exec_stream(&pod.ssh, &command).await
    }

    pub async fn known_models(&self) -> Result<KnownModelsReport> {
        let state = self.load_state().await?;
        let Some(pod) = state.active_pod.as_ref() else {
            return Err(anyhow!("no active pod configured"));
        };
        let catalog = self.load_catalog().await?;
        let (compatible, incompatible) = catalog.compatible_profiles(pod);

        Ok(KnownModelsReport {
            compatible: compatible
                .into_iter()
                .map(|profile| PodStatusDetail {
                    name: profile.name.clone(),
                    model: profile.model.clone(),
                    gpu_count: profile.gpu_count,
                })
                .collect(),
            incompatible: incompatible
                .into_iter()
                .map(|profile| PodStatusDetail {
                    name: profile.name.clone(),
                    model: profile.model.clone(),
                    gpu_count: profile.gpu_count,
                })
                .collect(),
        })
    }

    async fn persist_state(&self, state: &PodsState) -> Result<()> {
        self.store.save_state(state).await?;
        *self.cached_state.write() = Some(state.clone());
        Ok(())
    }

    async fn resolve_active_pod(
        &self,
        state: &mut PodsState,
        request: &PodStartRequest,
    ) -> Result<PodState> {
        let mut pod = state.active_pod.clone().unwrap_or_else(|| PodState {
            name: request
                .pod_name
                .clone()
                .unwrap_or_else(|| "active-pod".to_string()),
            ssh: request.ssh.clone().unwrap_or_default(),
            models_path: request.models_path.clone(),
            gpus: Vec::new(),
            models: BTreeMap::new(),
        });

        if let Some(name) = &request.pod_name {
            pod.name = name.clone();
        }
        if let Some(ssh) = &request.ssh {
            pod.ssh = ssh.clone();
        }
        if let Some(models_path) = &request.models_path {
            pod.models_path = Some(models_path.clone());
        }
        if !request.gpus.is_empty() {
            pod.gpus = request.gpus.clone();
        }

        if pod.ssh.is_empty() {
            return Err(anyhow!(
                "pod ssh command is required; pass --ssh or reuse the active pod"
            ));
        }
        if pod.gpus.is_empty() {
            return Err(anyhow!(
                "pod gpu inventory is required; pass --gpu entries or reuse the active pod"
            ));
        }

        state.active_pod = Some(pod.clone());
        self.persist_state(state).await?;
        Ok(pod)
    }

    fn resolve_profile(
        &self,
        catalog: &PodCatalog,
        pod: &PodState,
        request: &PodStartRequest,
    ) -> Result<PodProfile> {
        if let Some(profile_name) = request.profile.as_deref() {
            let profile = catalog
                .profiles
                .iter()
                .find(|profile| profile.name == profile_name)
                .cloned()
                .ok_or_else(|| anyhow!("unknown pod profile '{}'", profile_name))?;
            return Ok(profile);
        }

        let mut candidates = catalog.profiles_for_model(&request.model);
        candidates.retain(|profile| profile.matches_pod(pod));

        if let Some(requested_gpu_count) = request.requested_gpu_count {
            if let Some(profile) = candidates
                .iter()
                .copied()
                .find(|profile| profile.matches_gpu_count(requested_gpu_count))
            {
                return Ok(profile.clone());
            }

            if !candidates.is_empty() {
                let valid_counts = candidates
                    .iter()
                    .map(|profile| profile.gpu_count.to_string())
                    .collect::<Vec<_>>();
                return Err(anyhow!(
                    "no profile for '{}' with {} GPUs; valid counts: {}",
                    request.model,
                    requested_gpu_count,
                    valid_counts.join(", ")
                ));
            }
        }

        candidates
            .into_iter()
            .max_by_key(|profile| profile.gpu_count)
            .cloned()
            .or_else(|| {
                if request.model.is_empty() {
                    None
                } else {
                    Some(PodProfile {
                        name: request.model.clone(),
                        model: request.model.clone(),
                        gpu_count: request.requested_gpu_count.unwrap_or(1),
                        gpu_types: Vec::new(),
                        command_template: default_command_template(),
                        vllm_args: vec![
                            "--trust-remote-code".to_string(),
                            "--dtype".to_string(),
                            "auto".to_string(),
                        ],
                        env: BTreeMap::new(),
                    })
                }
            })
            .ok_or_else(|| anyhow!("no profile found for model '{}'", request.model))
    }

    async fn inspect_model(
        &self,
        pod: &PodState,
        name: &str,
        entry: &RunningModel,
    ) -> Result<PodHealth> {
        let process = self
            .transport
            .exec_capture(&pod.ssh, &format!("ps -p {}", entry.pid))
            .await?;
        let health = self
            .transport
            .exec_capture(
                &pod.ssh,
                &format!("curl -s -f http://localhost:{}/health", entry.port),
            )
            .await?;
        let log_tail = self
            .transport
            .exec_capture(
                &pod.ssh,
                &format!(
                    "tail -n 20 ~/{DEFAULT_LOG_DIR}/{}.log",
                    sanitize_component(name)
                ),
            )
            .await?;

        Ok(classify_status(
            process.success,
            health.success,
            &log_tail.stdout,
        ))
    }
}

impl Default for PodManager {
    fn default() -> Self {
        Self::new().expect("pod manager should initialize with a home directory")
    }
}

fn render_run_script(
    profile: &PodProfile,
    model: &str,
    name: &str,
    port: u16,
    vllm_args: &[String],
    gpu_ids: &[u32],
    models_path: Option<&str>,
) -> String {
    let mut script = String::new();
    script.push_str("#!/usr/bin/env bash\n");
    script.push_str("set -euo pipefail\n");
    script.push_str("export HF_HUB_ENABLE_HF_TRANSFER=1\n");
    script.push_str("export VLLM_NO_USAGE_STATS=1\n");
    script.push_str("export PYTORCH_CUDA_ALLOC_CONF=expandable_segments:True\n");
    script.push_str("export FORCE_COLOR=1\n");
    script.push_str("export TERM=xterm-256color\n");

    for (key, value) in &profile.env {
        script.push_str(&format!("export {key}={}\n", shell_quote(value)));
    }

    if gpu_ids.len() == 1 {
        script.push_str(&format!("export CUDA_VISIBLE_DEVICES={}\n", gpu_ids[0]));
    }

    if let Some(models_path) = models_path {
        script.push_str(&format!(
            "export MODELS_PATH={}\n",
            shell_quote(models_path)
        ));
    }

    let command = render_template(
        &profile.command_template,
        model,
        name,
        port,
        &join_args(vllm_args),
    );
    script.push_str(&format!("exec {command}\n"));
    script
}

fn render_wrapper_script(run_path: &str, log_path: &str) -> String {
    format!(
        "#!/usr/bin/env bash\nset -euo pipefail\nmkdir -p ~/.vllm_logs\nscript -q -f -c {run_path} {log_path}\n"
    )
}

fn render_template(template: &str, model: &str, name: &str, port: u16, vllm_args: &str) -> String {
    template
        .replace("{{MODEL_ID}}", model)
        .replace("{{NAME}}", name)
        .replace("{{PORT}}", &port.to_string())
        .replace("{{VLLM_ARGS}}", vllm_args)
}

fn join_args(args: &[String]) -> String {
    args.iter()
        .map(|value| shell_quote(value))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }

    if value.chars().all(|ch| {
        ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | ',' | '=' | '@')
    }) {
        return value.to_string();
    }

    format!("'{}'", value.replace('\'', r#"'"'"'"#))
}

fn select_gpus(pod: &PodState, count: usize) -> Vec<u32> {
    if count >= pod.gpus.len() {
        return pod.gpus.iter().map(|gpu| gpu.id).collect();
    }

    let mut usage: BTreeMap<u32, usize> = pod.gpus.iter().map(|gpu| (gpu.id, 0)).collect();
    for model in pod.models.values() {
        for gpu_id in &model.gpu_ids {
            if let Some(slot) = usage.get_mut(gpu_id) {
                *slot += 1;
            }
        }
    }

    let mut gpus = pod.gpus.clone();
    gpus.sort_by_key(|gpu| (usage.get(&gpu.id).copied().unwrap_or(0), gpu.id));
    gpus.into_iter().take(count).map(|gpu| gpu.id).collect()
}

fn next_port(pod: &PodState) -> u16 {
    let mut port = DEFAULT_START_PORT;
    let occupied: std::collections::HashSet<u16> =
        pod.models.values().map(|model| model.port).collect();
    while occupied.contains(&port) {
        port = port.saturating_add(1);
    }
    port
}

fn parse_pid(output: &str) -> Result<u32> {
    let pid = output
        .trim()
        .lines()
        .find_map(|line| line.trim().parse::<u32>().ok())
        .ok_or_else(|| anyhow!("launch command did not return a pid"))?;
    Ok(pid)
}

fn sanitize_component(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "model".to_string()
    } else {
        out
    }
}

fn classify_status(process_alive: bool, health_ok: bool, log_tail: &str) -> PodHealth {
    if process_alive && health_ok {
        return PodHealth::Running;
    }

    let lower = log_tail.to_lowercase();
    let failed = lower.contains("model runner exiting with code")
        || lower.contains("script exited with code")
        || lower.contains("torch.outofmemoryerror")
        || lower.contains("cuda out of memory")
        || lower.contains("runtimeerror: engine core initialization failed");

    if failed {
        PodHealth::Crashed
    } else if process_alive {
        PodHealth::Starting
    } else {
        PodHealth::Dead
    }
}

fn render_args(args: &[String], memory: Option<f32>, context: Option<&str>) -> Result<Vec<String>> {
    let mut rendered = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let arg = &args[index];
        if arg == "--gpu-memory-utilization" {
            index += 2;
            continue;
        }
        if arg == "--max-model-len" {
            index += 2;
            continue;
        }
        rendered.push(arg.clone());
        index += 1;
    }

    if let Some(memory) = memory {
        let utilization = (memory / 100.0).clamp(0.0, 1.0);
        rendered.push("--gpu-memory-utilization".to_string());
        rendered.push(format!("{utilization:.2}"));
    }

    if let Some(context) = context {
        rendered.push("--max-model-len".to_string());
        rendered.push(parse_context_size(context)?.to_string());
    }

    Ok(rendered)
}

fn parse_context_size(value: &str) -> Result<u32> {
    let trimmed = value.trim().to_lowercase();
    if let Some(stripped) = trimmed.strip_suffix('k') {
        return Ok(stripped
            .parse::<u32>()
            .with_context(|| format!("invalid context size '{value}'"))?
            .saturating_mul(1024));
    }

    trimmed
        .parse::<u32>()
        .with_context(|| format!("invalid context size '{value}'"))
}

fn default_command_template() -> String {
    "vllm serve {{MODEL_ID}} --served-model-name {{NAME}} --port {{PORT}} {{VLLM_ARGS}}".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pods::catalog::PodProfile;
    use crate::pods::state::PodGpu;
    use crate::pods::transport::CommandOutput;
    use anyhow::Result;
    use async_trait::async_trait;
    use parking_lot::Mutex;
    use std::collections::VecDeque;

    #[derive(Clone, Default)]
    struct MockTransport {
        commands: Arc<Mutex<Vec<String>>>,
        writes: Arc<Mutex<Vec<(String, String)>>>,
        responses: Arc<Mutex<VecDeque<CommandOutput>>>,
    }

    #[async_trait]
    impl PodTransport for MockTransport {
        async fn exec_capture(&self, _ssh_target: &str, command: &str) -> Result<CommandOutput> {
            self.commands.lock().push(command.to_string());
            Ok(self
                .responses
                .lock()
                .pop_front()
                .unwrap_or_else(|| CommandOutput {
                    success: true,
                    stdout: "12345\n".to_string(),
                    stderr: String::new(),
                }))
        }

        async fn write_file(
            &self,
            _ssh_target: &str,
            remote_path: &str,
            contents: &str,
        ) -> Result<()> {
            self.writes
                .lock()
                .push((remote_path.to_string(), contents.to_string()));
            Ok(())
        }

        async fn exec_stream(&self, _ssh_target: &str, command: &str) -> Result<()> {
            self.commands.lock().push(command.to_string());
            Ok(())
        }
    }

    #[test]
    fn context_parser_handles_shorthand() {
        assert_eq!(parse_context_size("32k").expect("parse"), 32_768);
        assert_eq!(parse_context_size("131072").expect("parse"), 131_072);
    }

    #[test]
    fn classify_status_detects_failure_patterns() {
        assert_eq!(
            classify_status(
                true,
                false,
                "RuntimeError: Engine core initialization failed"
            ),
            PodHealth::Crashed
        );
        assert_eq!(classify_status(false, false, ""), PodHealth::Dead);
        assert_eq!(classify_status(true, false, ""), PodHealth::Starting);
    }

    #[test]
    fn select_gpus_prefers_less_loaded_devices() {
        let pod = PodState {
            name: "pod".to_string(),
            ssh: "ssh root@example.com".to_string(),
            models_path: None,
            gpus: vec![
                PodGpu {
                    id: 0,
                    name: "A100".to_string(),
                },
                PodGpu {
                    id: 1,
                    name: "A100".to_string(),
                },
            ],
            models: BTreeMap::from([(
                "existing".to_string(),
                RunningModel {
                    model: "model".to_string(),
                    port: 8001,
                    gpu_ids: vec![0],
                    pid: 1,
                    profile: "profile".to_string(),
                },
            )]),
        };

        assert_eq!(select_gpus(&pod, 1), vec![1]);
    }

    #[tokio::test]
    async fn render_and_launch_flow_updates_state() {
        let store = PodsStore::new(
            std::env::temp_dir().join(format!("vtcode-pods-start-test-{}", std::process::id())),
        );
        let transport = Arc::new(MockTransport::default());
        let manager = PodManager::with_transport(store, transport.clone());
        let state = PodsState {
            version: env!("CARGO_PKG_VERSION").to_string(),
            active_pod: Some(PodState {
                name: "gpu-box".to_string(),
                ssh: "ssh root@example.com".to_string(),
                models_path: Some("/models".to_string()),
                gpus: vec![PodGpu {
                    id: 0,
                    name: "A100".to_string(),
                }],
                models: BTreeMap::new(),
            }),
        };
        manager.store.save_state(&state).await.expect("save");
        manager
            .store
            .save_catalog(&PodCatalog {
                version: "1".to_string(),
                profiles: vec![PodProfile {
                    name: "test".to_string(),
                    model: "test/model".to_string(),
                    gpu_count: 1,
                    gpu_types: vec!["A100".to_string()],
                    command_template: default_command_template(),
                    vllm_args: vec!["--max-model-len".to_string(), "4096".to_string()],
                    env: BTreeMap::new(),
                }],
            })
            .await
            .expect("save catalog");

        let result = manager
            .start_model(PodStartRequest {
                pod_name: None,
                ssh: None,
                gpus: Vec::new(),
                models_path: None,
                name: "local".to_string(),
                model: "test/model".to_string(),
                profile: None,
                requested_gpu_count: None,
                memory: Some(75.0),
                context: Some("4k".to_string()),
            })
            .await
            .expect("start");

        assert_eq!(result.entry.pid, 12345);
        assert!(
            transport
                .writes
                .lock()
                .iter()
                .any(|(path, contents)| path.contains("model_run_local.sh")
                    && contents.contains("vllm serve"))
        );
    }

    #[test]
    fn memory_override_rewrites_existing_argument() {
        let args = render_args(
            &[
                "--dtype".to_string(),
                "auto".to_string(),
                "--gpu-memory-utilization".to_string(),
                "0.80".to_string(),
            ],
            Some(90.0),
            None,
        )
        .expect("render args");

        assert!(
            args.windows(2)
                .any(|pair| pair == ["--gpu-memory-utilization", "0.90"])
        );
    }
}
