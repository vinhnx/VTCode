use super::*;

/// Distinguishes how facts are provided to the persistence layer.
pub(super) enum FactsInput<'a> {
    /// Facts already classified into topics (no LLM call needed).
    Preclassified(&'a [GroundedFactRecord]),
    /// Raw candidate facts that must be classified via LLM.
    Candidates(&'a [GroundedFactRecord]),
}

impl FactsInput<'_> {
    fn as_slice(&self) -> &[GroundedFactRecord] {
        match self {
            FactsInput::Preclassified(facts) => facts,
            FactsInput::Candidates(facts) => facts,
        }
    }
}

pub(super) async fn persist_memory_internal(
    config: &PersistentMemoryConfig,
    workspace_root: &Path,
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    facts_input: FactsInput<'_>,
    write_rollout: bool,
    force_rebuild: bool,
) -> Result<Option<PersistentMemoryWriteReport>> {
    let cfg = config.clone();
    let ws = workspace_root.to_path_buf();
    let directory = tokio::task::spawn_blocking(move || resolve_persistent_memory_dir(&cfg, &ws))
        .await
        .context("Persistent memory directory resolution task panicked")??
        .context("persistent memory directory should resolve when enabled")?;
    let files = PersistentMemoryFiles::new(directory, config.directory_override.is_none());
    let mut created_files = Vec::new();
    ensure_memory_layout(&files, &mut created_files).await?;

    let facts_slice = facts_input.as_slice();
    if detect_memory_cleanup_status_async(&files).await?.needed && (write_rollout || !facts_slice.is_empty()) {
        bail!("persistent memory cleanup is required before mutating memory");
    }

    let _lock = MemoryLock::acquire(&files.lock_file).await?;
    let existing_lines = read_existing_memory_lines(&files.directory).await?;
    let deduped_records: Vec<GroundedFactRecord> = facts_slice
        .iter()
        .filter(|f| !existing_lines.contains(&normalize_whitespace(&f.fact).to_ascii_lowercase()))
        .cloned()
        .collect();

    let classified = match facts_input {
        FactsInput::Preclassified(_) => classified_facts_from_records(&deduped_records),
        FactsInput::Candidates(_) if deduped_records.is_empty() => ClassifiedFacts {
            preferences: Vec::new(),
            repository_facts: Vec::new(),
        },
        FactsInput::Candidates(_) => {
            classify_facts_strict(runtime_config, vt_cfg, workspace_root, &deduped_records).await?
        }
    };

    let staged_rollout = if write_rollout && classified.total() > 0 {
        Some(
            write_rollout_summary_pending(&files.rollout_summaries_dir, &classified, files.private_global)
                .await
                .with_context(|| {
                    format!("Failed to write rollout summary under {}", files.rollout_summaries_dir.display())
                })?,
        )
    } else {
        None
    };

    let pending_before = list_pending_rollout_files_async(&files.rollout_summaries_dir).await?;
    let (summary_exists, memory_exists) =
        tokio::try_join!(tokio::fs::try_exists(&files.summary_file), tokio::fs::try_exists(&files.memory_file),)?;
    let should_consolidate =
        force_rebuild || staged_rollout.is_some() || !pending_before.is_empty() || !summary_exists || !memory_exists;
    if !should_consolidate {
        return Ok(None);
    }

    let consolidated = consolidate_memory_files(runtime_config, vt_cfg, workspace_root, &files).await?;
    created_files.extend(consolidated.created_files);
    created_files.sort();
    created_files.dedup();

    Ok(Some(PersistentMemoryWriteReport {
        directory: files.directory,
        summary_file: files.summary_file,
        memory_file: files.memory_file,
        rollout_summary_file: staged_rollout.map(finalize_rollout_summary_path),
        created_files,
        added_facts: consolidated.added_facts,
        pending_rollout_summaries: count_pending_rollout_summaries_async(&files.rollout_summaries_dir).await?,
    }))
}

fn classified_facts_from_records(records: &[GroundedFactRecord]) -> ClassifiedFacts {
    let mut preferences = Vec::new();
    let mut repository_facts = Vec::new();
    for fact in records {
        let topic = decode_topic_source(&fact.source).0.unwrap_or_else(|| classify_fact(fact));
        match topic {
            MemoryTopic::Preferences => preferences.push(fact.clone()),
            MemoryTopic::RepositoryFacts => repository_facts.push(fact.clone()),
        }
    }
    ClassifiedFacts {
        preferences: merge_topic_facts(preferences),
        repository_facts: merge_topic_facts(repository_facts),
    }
}

pub(super) async fn ensure_memory_layout(
    files: &PersistentMemoryFiles,
    created_files: &mut Vec<PathBuf>,
) -> Result<()> {
    async fn ensure_file(
        path: &Path,
        contents: String,
        created_files: &mut Vec<PathBuf>,
        private_global: bool,
    ) -> Result<()> {
        if private_global {
            match tokio::fs::symlink_metadata(path).await {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!("Refusing symlinked persistent memory file {}", path.display());
                }
                Ok(metadata) if !metadata.is_file() => {
                    bail!("Persistent memory path is not a regular file: {}", path.display());
                }
                Ok(_) => return Ok(()),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(error).with_context(|| format!("Failed to inspect {}", path.display()));
                }
            }
        } else if tokio::fs::try_exists(path).await.unwrap_or(false) {
            return Ok(());
        }
        if private_global {
            vtcode_commons::fs::write_private_file_atomic(path, contents.as_bytes())
                .await
                .with_context(|| format!("Failed to write private memory file {}", path.display()))?;
        } else {
            tokio::fs::write(path, contents)
                .await
                .with_context(|| format!("Failed to write {}", path.display()))?;
        }
        created_files.push(path.to_path_buf());
        Ok(())
    }
    for (dir, desc) in [
        (&files.directory, "persistent memory"),
        (&files.rollout_summaries_dir, "rollout summaries"),
        (&files.notes_dir, "notes"),
    ] {
        if files.private_global {
            VtCodePaths::ensure_user_dir(dir).with_context(|| format!("Failed to create {desc} {}", dir.display()))?;
        } else {
            tokio::fs::create_dir_all(dir)
                .await
                .with_context(|| format!("Failed to create {desc} {}", dir.display()))?;
        }
    }
    ensure_file(
        &files.preferences_file,
        render_topic_file(MemoryTopic::Preferences, &[]),
        created_files,
        files.private_global,
    )
    .await?;
    ensure_file(
        &files.repository_facts_file,
        render_topic_file(MemoryTopic::RepositoryFacts, &[]),
        created_files,
        files.private_global,
    )
    .await?;
    ensure_file(&files.memory_file, render_memory_index(&[], &[], &[], 0), created_files, files.private_global).await?;
    ensure_file(&files.summary_file, render_memory_summary(&[], &[], &[]), created_files, files.private_global).await?;
    Ok(())
}

pub(super) fn is_regular_file(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|metadata| metadata.is_file() && !metadata.file_type().is_symlink())
}

pub(super) fn truncate_memory_excerpt(
    contents: &str,
    line_limit: usize,
    byte_limit: usize,
) -> (String, bool, usize, usize) {
    let all_lines = contents.lines().collect::<Vec<_>>();
    let mut selected = String::new();
    let mut bytes_read = 0usize;
    let mut lines_read = 0usize;
    let mut truncated = false;
    for (index, line) in all_lines.iter().enumerate() {
        if lines_read >= line_limit {
            truncated = true;
            break;
        }
        let line_bytes = line.len();
        let trailing_newline = usize::from(index + 1 < all_lines.len());
        if bytes_read + line_bytes + trailing_newline > byte_limit {
            truncated = true;
            break;
        }
        selected.push_str(line);
        selected.push('\n');
        bytes_read += line_bytes + trailing_newline;
        lines_read += 1;
    }
    if !truncated && contents.len() > bytes_read {
        truncated = true;
    }
    (selected.trim_end().to_string(), truncated, bytes_read, lines_read)
}

async fn read_existing_memory_lines(directory: &Path) -> Result<BTreeSet<String>> {
    let mut lines = BTreeSet::new();
    if !tokio::fs::try_exists(directory).await.unwrap_or(false) {
        return Ok(lines);
    }
    let mut stack = vec![directory.to_path_buf()];
    while let Some(next_dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&next_dir)
            .await
            .with_context(|| format!("Failed to list {}", next_dir.display()))?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if entry.metadata().await?.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|v| v.to_str()) != Some("md") {
                continue;
            }
            let content = String::from_utf8(vtcode_commons::fs::read_private_file_no_follow(&path).await?)
                .with_context(|| format!("failed to decode note file at {}", path.display()))?;
            for line in content.lines() {
                if let Some((_, fact)) = parse_fact_line(line) {
                    lines.insert(normalize_whitespace(&fact).to_ascii_lowercase());
                }
            }
        }
    }
    Ok(lines)
}

const CLEANUP_REMEMBER_MARKERS: &[&str] = &[
    "save to memory",
    "remember that",
    "remember my",
    "remember ",
    "add to memory",
    "store in memory",
];
const CLEANUP_FORGET_MARKERS: &[&str] = &["forget ", "remove from memory", "delete from memory"];
pub(super) const STRIP_PREFIXES: &[&str] = &[
    "please ",
    "please, ",
    "can you ",
    "could you ",
    "would you ",
    "vt code, ",
    "vt code ",
];
pub(super) const CLEANUP_NOTE_PREFIXES: &[&str] = &["note that ", "important:"];
pub(super) const SELF_FACT_PREFIXES: &[&str] = &[
    "my name is ",
    "i prefer ",
    "my preferred ",
    "my pronouns are ",
    "my timezone is ",
];

pub(super) fn detect_memory_cleanup_status(files: &PersistentMemoryFiles) -> Result<MemoryCleanupStatus> {
    if !files.directory.exists() {
        return Ok(MemoryCleanupStatus {
            needed: false,
            suspicious_facts: 0,
            suspicious_summary_lines: 0,
        });
    }
    let mut suspicious_facts = 0usize;
    for path in [
        &files.preferences_file,
        &files.repository_facts_file,
        &files.memory_file,
    ] {
        suspicious_facts += count_suspicious_facts_in_file(path)?;
    }
    suspicious_facts += count_suspicious_rollout_facts(&files.rollout_summaries_dir)?;
    let suspicious_summary_lines = count_suspicious_summary_lines(&files.summary_file)?;
    Ok(MemoryCleanupStatus {
        needed: suspicious_facts > 0 || suspicious_summary_lines > 0,
        suspicious_facts,
        suspicious_summary_lines,
    })
}

pub(super) async fn detect_memory_cleanup_status_async(files: &PersistentMemoryFiles) -> Result<MemoryCleanupStatus> {
    let files = files.clone();
    tokio::task::spawn_blocking(move || detect_memory_cleanup_status(&files))
        .await
        .context("Persistent memory cleanup status task panicked")?
}

fn count_suspicious_facts_in_file(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let content = String::from_utf8(VtCodePaths::read_file_no_follow(path)?)
        .with_context(|| format!("Failed to decode {} as UTF-8", path.display()))?;
    Ok(parse_topic_file(&content).into_iter().filter(is_legacy_polluted_fact).count())
}

fn count_suspicious_rollout_facts(rollout_dir: &Path) -> Result<usize> {
    if !rollout_dir.exists() {
        return Ok(0);
    }
    let mut count = 0usize;
    for entry in std::fs::read_dir(rollout_dir).with_context(|| format!("Failed to list {}", rollout_dir.display()))? {
        let path = entry?.path();
        if path.extension().and_then(|v| v.to_str()) == Some("md") {
            count += count_suspicious_facts_in_file(&path)?;
        }
    }
    Ok(count)
}

fn count_suspicious_summary_lines(path: &Path) -> Result<usize> {
    if !path.exists() {
        return Ok(0);
    }
    let content = String::from_utf8(VtCodePaths::read_file_no_follow(path)?)
        .with_context(|| format!("Failed to decode {} as UTF-8", path.display()))?;
    Ok(content
        .lines()
        .map(str::trim)
        .filter(|l| l.starts_with("- "))
        .map(|l| l.trim_start_matches("- ").trim())
        .filter(|l| looks_like_legacy_prompt(l) || looks_like_serialized_payload(l))
        .count())
}

#[cold]
fn is_legacy_polluted_fact(fact: &GroundedFactRecord) -> bool {
    looks_like_legacy_prompt(&fact.fact) || looks_like_serialized_payload(&fact.fact)
}

#[cold]
pub(super) fn looks_like_legacy_prompt(text: &str) -> bool {
    let mut lowered = normalize_whitespace(text).to_ascii_lowercase();
    while let Some(stripped) = STRIP_PREFIXES.iter().find_map(|p| lowered.strip_prefix(p)) {
        lowered = stripped.trim_start().to_string();
    }
    CLEANUP_REMEMBER_MARKERS
        .iter()
        .chain(CLEANUP_FORGET_MARKERS.iter())
        .any(|m| lowered.starts_with(m))
}

#[cold]
fn looks_like_serialized_payload(text: &str) -> bool {
    let t = text.trim();
    t.starts_with('{')
        || t.starts_with('[')
        || t.contains("\"query\":")
        || t.contains("\"matches\":")
        || t.contains("\"path\":")
        || t.contains("</parameter>")
        || t.contains("</invoke>")
        || t.contains("<</invoke>")
}

async fn write_rollout_summary_pending(
    rollout_dir: &Path,
    classified: &ClassifiedFacts,
    private_global: bool,
) -> Result<PathBuf> {
    if private_global {
        VtCodePaths::ensure_user_dir(rollout_dir)
            .with_context(|| format!("Failed to create {}", rollout_dir.display()))?;
    } else {
        tokio::fs::create_dir_all(rollout_dir)
            .await
            .with_context(|| format!("Failed to create {}", rollout_dir.display()))?;
    }
    let path = rollout_dir.join(format!("{}.pending.md", unique_rollout_id()));
    let contents = render_rollout_summary(classified);
    if private_global {
        vtcode_commons::fs::write_private_file_atomic(&path, contents.as_bytes())
            .await
            .with_context(|| format!("Failed to write private memory rollout summary {}", path.display()))?;
    } else {
        tokio::fs::write(&path, contents)
            .await
            .with_context(|| format!("Failed to write {}", path.display()))?;
    }
    Ok(path)
}

fn finalize_rollout_summary_path(path: PathBuf) -> PathBuf {
    match path.file_name().and_then(|v| v.to_str()) {
        Some(name) => path.with_file_name(name.trim_end_matches(".pending.md").to_string() + ".md"),
        None => path,
    }
}

pub(super) struct ConsolidationResult {
    created_files: Vec<PathBuf>,
    added_facts: usize,
}

pub(super) async fn consolidate_memory_files(
    runtime_config: Option<&RuntimeAgentConfig>,
    vt_cfg: Option<&VTCodeConfig>,
    workspace_root: &Path,
    files: &PersistentMemoryFiles,
) -> Result<ConsolidationResult> {
    let (pending_files, prefs_existing, repo_existing, rollout, notes) = tokio::try_join!(
        list_pending_rollout_files_async(&files.rollout_summaries_dir),
        read_topic_records(&files.preferences_file, MemoryTopic::Preferences),
        read_topic_records(&files.repository_facts_file, MemoryTopic::RepositoryFacts),
        read_rollout_records(&files.rollout_summaries_dir),
        read_note_summaries(&files.notes_dir),
    )?;
    let classified = ClassifiedFacts {
        preferences: merge_topic_facts(prefs_existing.into_iter().chain(rollout.0).collect()),
        repository_facts: merge_topic_facts(repo_existing.into_iter().chain(rollout.1).collect()),
    };
    let created_files =
        write_classified_memory(files, &classified, &notes, runtime_config, vt_cfg, workspace_root).await?;
    let mut added_facts = 0usize;
    for p in &pending_files {
        if let Ok(c) = String::from_utf8(vtcode_commons::fs::read_private_file_no_follow(p).await?) {
            added_facts += c.lines().filter_map(parse_fact_line).count();
        }
    }
    for pending in &pending_files {
        let finalized = finalize_rollout_summary_path(pending.clone());
        if !tokio::fs::try_exists(&finalized).await.unwrap_or(false) {
            tokio::fs::rename(pending, &finalized)
                .await
                .with_context(|| format!("Failed to finalize rollout summary {}", pending.display()))?;
        } else {
            tokio::fs::remove_file(pending)
                .await
                .with_context(|| format!("Failed to remove {}", pending.display()))?;
        }
    }
    Ok(ConsolidationResult { created_files, added_facts })
}

fn merge_topic_facts(records: Vec<GroundedFactRecord>) -> Vec<GroundedFactRecord> {
    let mut facts = Vec::new();
    for fact in records {
        let normalized = normalize_whitespace(&fact.fact).to_ascii_lowercase();
        if let Some(i) = facts
            .iter()
            .position(|e: &GroundedFactRecord| normalize_whitespace(&e.fact).to_ascii_lowercase() == normalized)
        {
            facts.remove(i);
        }
        facts.push(fact);
    }
    let skip = facts.len().saturating_sub(TOPIC_FACT_LIMIT);
    facts.into_iter().skip(skip).collect()
}

fn normalized_selection_key(source: &str, fact: &str) -> String {
    format!(
        "{}::{}",
        normalize_whitespace(source).to_ascii_lowercase(),
        normalize_whitespace(fact).to_ascii_lowercase()
    )
}

fn selection_key_for_record(record: &GroundedFactRecord) -> String {
    let (_topic, source) = decode_topic_source(&record.source);
    normalized_selection_key(&source, &record.fact)
}

fn selection_keys(selected: &[MemoryOpCandidate]) -> BTreeSet<String> {
    selected.iter().map(|e| normalized_selection_key(&e.source, &e.fact)).collect()
}

pub(super) async fn rewrite_topic_without_selected(
    path: &Path,
    topic: MemoryTopic,
    selected: &[MemoryOpCandidate],
    private_global: bool,
) -> Result<usize> {
    if !tokio::fs::try_exists(path).await.unwrap_or(false) {
        return Ok(0);
    }
    let keys = selection_keys(selected);
    let facts = read_topic_records(path, topic).await?;
    let removed = facts.iter().filter(|f| keys.contains(&selection_key_for_record(f))).count();
    if removed == 0 {
        return Ok(0);
    }
    let kept: Vec<_> = facts
        .into_iter()
        .filter(|f| !keys.contains(&selection_key_for_record(f)))
        .collect();
    let contents = render_topic_file(topic, &kept);
    if private_global {
        vtcode_commons::fs::write_private_file_atomic(path, contents.as_bytes())
            .await
            .with_context(|| format!("Failed to write private memory file {}", path.display()))?;
    } else {
        tokio::fs::write(path, contents)
            .await
            .with_context(|| format!("Failed to write {}", path.display()))?;
    }
    Ok(removed)
}

pub(super) async fn scrub_rollout_file_by_selection(
    path: &Path,
    selected: &[MemoryOpCandidate],
    private_global: bool,
) -> Result<usize> {
    let contents = String::from_utf8(vtcode_commons::fs::read_private_file_no_follow(path).await?)
        .with_context(|| format!("Failed to decode {} as UTF-8", path.display()))?;
    let keys = selection_keys(selected);
    let mut removed = 0usize;
    let mut filtered = Vec::new();
    for line in contents.lines() {
        let keep = parse_fact_line(line).is_none_or(|(source, fact)| {
            let m = keys.contains(&selection_key_for_record(&GroundedFactRecord { source, fact }));
            if m {
                removed += 1;
            }
            !m
        });
        if keep {
            filtered.push(line);
        }
    }
    if removed == 0 {
        return Ok(0);
    }
    let mut rewritten = filtered.join("\n");
    if contents.ends_with('\n') {
        rewritten.push('\n');
    }
    if private_global {
        vtcode_commons::fs::write_private_file_atomic(path, rewritten.as_bytes())
            .await
            .with_context(|| format!("Failed to write private memory rollout summary {}", path.display()))?;
    } else {
        tokio::fs::write(path, rewritten)
            .await
            .with_context(|| format!("Failed to write {}", path.display()))?;
    }
    Ok(removed)
}

pub(super) async fn remove_rollout_markdown_files(rollout_dir: &Path) -> Result<usize> {
    let files = list_rollout_markdown_files(rollout_dir)?;
    let count = files.len();
    for p in files {
        tokio::fs::remove_file(&p)
            .await
            .with_context(|| format!("Failed to remove {}", p.display()))?;
    }
    Ok(count)
}

pub(super) fn memory_plan_facts(plan: &MemoryOpPlan) -> Result<Vec<GroundedFactRecord>> {
    if plan.kind != MemoryOpKind::Remember {
        bail!("memory plan is not a remember operation");
    }
    Ok(plan
        .facts
        .iter()
        .map(|f| {
            let topic = match f.topic {
                MemoryPlannedTopic::Preferences => MemoryTopic::Preferences,
                MemoryPlannedTopic::RepositoryFacts => MemoryTopic::RepositoryFacts,
            };
            let source = if f.source.trim().is_empty() {
                "manual_memory".to_string()
            } else {
                normalize_whitespace(&f.source)
            };
            GroundedFactRecord {
                fact: truncate_for_fact(&normalize_whitespace(&f.fact), 180),
                source: encode_topic_source(topic, &source),
            }
        })
        .filter(|f| !f.fact.is_empty())
        .collect())
}

pub(super) fn selected_memory_candidates(
    candidates: &[MemoryOpCandidate],
    selected_ids: &[usize],
) -> Result<Vec<MemoryOpCandidate>> {
    let selected: Vec<_> = selected_ids
        .iter()
        .filter_map(|id| candidates.iter().find(|c| c.id == *id).cloned())
        .collect();
    if selected_ids.len() != selected.len() {
        bail!("memory plan selected a missing candidate");
    }
    Ok(selected)
}
