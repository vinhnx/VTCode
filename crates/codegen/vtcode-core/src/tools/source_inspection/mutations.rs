use super::*;
use crate::tools::PatchError;
use crate::tools::editing::{Patch, PatchOperation};

/// A resolved replacement from the same matcher used by patch application.
pub(crate) struct InspectedReplacement {
    pub(crate) start: usize,
    pub(crate) removed: usize,
    pub(crate) added: usize,
}

pub(crate) struct PatchInspection {
    pub(crate) replacements: Vec<InspectedReplacement>,
    pub(crate) required_ranges: Vec<Range<usize>>,
}

pub(crate) enum PreparedPatchCredit {
    Created {
        path: PathBuf,
        content: String,
    },
    Removed {
        path: PathBuf,
    },
    Updated {
        source: PathBuf,
        destination: PathBuf,
        before: String,
        after: String,
        replacements: Vec<InspectedReplacement>,
    },
}

impl SourceInspection {
    pub(crate) fn record_edit(
        &self,
        path: &Path,
        before: &str,
        after: &str,
        replacements: &[InspectedReplacement],
        support: String,
    ) {
        if self.mode() == InspectionMode::Off || before == after {
            return;
        }
        let support = self.register_support(support);
        let mut ledger = self.0.lock();
        let Some(mut file) = ledger.files.remove(path) else {
            return;
        };
        ledger.order.retain(|entry| entry != path);
        if file.revision != revision(before) {
            return;
        }
        let mut dependencies = BTreeSet::from([support.clone()]);
        for credit in file
            .lines
            .values()
            .filter(|credit| credit.iter().all(|key| ledger.active.contains(key)))
        {
            dependencies.extend(credit.iter().cloned());
        }
        for replacement in replacements {
            let start = replacement.start.saturating_sub(1);
            let end = (replacement.start + replacement.removed + 1).min(file.line_count);
            for index in start..end {
                if let Some(credit) = file.lines.get(&index) {
                    dependencies.extend(credit.iter().cloned());
                }
            }
        }
        if let Some(credit) = &file.empty {
            dependencies.extend(credit.iter().cloned());
        }
        if dependencies.len() > MAX_SUPPORTS {
            return;
        }
        let authored_credit = Arc::new(dependencies);
        let mut mapped = HashMap::new();
        for (index, credit) in file.lines.drain() {
            let mut offset = 0isize;
            let mut survives = true;
            for replacement in replacements {
                if index >= replacement.start && index < replacement.start + replacement.removed {
                    survives = false;
                    break;
                }
                if index >= replacement.start + replacement.removed {
                    offset += replacement.added as isize - replacement.removed as isize;
                }
            }
            if survives && let Some(index) = index.checked_add_signed(offset) {
                let mut dependencies = credit.as_ref().clone();
                dependencies.insert(support.clone());
                if dependencies.len() <= MAX_SUPPORTS {
                    mapped.insert(index, Arc::new(dependencies));
                }
            }
        }
        let mut offset = 0isize;
        for replacement in replacements {
            if let Some(start) = replacement.start.checked_add_signed(offset) {
                for index in start..start.saturating_add(replacement.added) {
                    if mapped.len() >= MAX_CREDIT_LINES {
                        break;
                    }
                    mapped.insert(index, Arc::clone(&authored_credit));
                }
            }
            offset += replacement.added as isize - replacement.removed as isize;
        }
        file.lines = mapped;
        file.revision = revision(after);
        file.line_count = after.lines().count();
        file.empty = after.is_empty().then_some(authored_credit);
        ledger.order.push_back(path.to_path_buf());
        ledger.files.insert(path.to_path_buf(), file);
    }

    pub(crate) fn record_created(&self, path: &Path, content: &str, support: String) {
        if self.mode() == InspectionMode::Off {
            return;
        }
        let support = self.register_support(support);
        self.insert_lines(
            path.to_path_buf(),
            content,
            (0..content.lines().count().min(MAX_CREDIT_LINES)).collect(),
            support,
            content.is_empty(),
        );
    }

    pub(crate) fn record_transfer(
        &self,
        source: &Path,
        destination: &Path,
        content: &str,
        support: String,
        moved: bool,
    ) {
        if self.mode() == InspectionMode::Off {
            return;
        }
        let support = self.register_support(support);
        let mut ledger = self.0.lock();
        let Some(source_file) = ledger.files.get(source) else {
            return;
        };
        if source_file.revision != revision(content) {
            return;
        }
        let mut transferred = FileEvidence {
            revision: source_file.revision.clone(),
            line_count: source_file.line_count,
            ..FileEvidence::default()
        };
        for (line, credit) in &source_file.lines {
            let mut dependencies = credit.as_ref().clone();
            dependencies.insert(support.clone());
            if dependencies.len() <= MAX_SUPPORTS {
                transferred.lines.insert(*line, Arc::new(dependencies));
            }
        }
        if let Some(credit) = &source_file.empty {
            let mut dependencies = credit.as_ref().clone();
            dependencies.insert(support);
            if dependencies.len() <= MAX_SUPPORTS {
                transferred.empty = Some(Arc::new(dependencies));
            }
        }
        if moved {
            ledger.files.remove(source);
            ledger.order.retain(|entry| entry != source);
        }
        if ledger.files.len() >= MAX_FILES
            && let Some(oldest) = ledger.order.pop_front()
        {
            ledger.files.remove(&oldest);
        }
        ledger.order.retain(|path| path != destination);
        ledger.order.push_back(destination.to_path_buf());
        ledger.files.insert(destination.to_path_buf(), transferred);
    }

    pub(crate) async fn acquire_patch_leases(
        monitor: &crate::tools::edited_file_monitor::EditedFileMonitor,
        patch: &Patch,
        root: &Path,
    ) -> Result<Vec<crate::tools::edited_file_monitor::MutationLease>> {
        let mut paths = Vec::new();
        for target in crate::tools::apply_patch::patch_mutation_target_paths(patch) {
            paths.push(vtcode_commons::paths::ensure_path_within_workspace_resolved(&root.join(target), root).await?);
        }
        paths.sort();
        paths.dedup();
        let mut leases = Vec::new();
        for path in paths {
            leases.push(monitor.acquire_mutation(&path).await);
        }
        Ok(leases)
    }

    /// Validate every operation before any operation writes. Internal source
    /// loading is only for matching and revision checks, never authorization.
    pub(crate) async fn prepare_patch(&self, patch: &Patch, root: &Path) -> Result<Vec<PreparedPatchCredit>> {
        if self.mode() == InspectionMode::Off {
            return Ok(Vec::new());
        }
        let mut prepared = Vec::new();
        for operation in patch.operations() {
            match operation {
                PatchOperation::AddFile { path, content } => {
                    let path =
                        vtcode_commons::paths::ensure_path_within_workspace_resolved(&root.join(path), root).await?;
                    if tokio::fs::try_exists(&path).await? {
                        // Typed error: the canonical planner produces the same
                        // "invalid patch operation" contract, so pre-write
                        // inspection must not replace it with a plain string.
                        return Err(PatchError::InvalidOperation {
                            path: path.to_string_lossy().into_owned(),
                            reason: "new file already exists".to_owned(),
                        }
                        .into());
                    }
                    prepared.push(PreparedPatchCredit::Created { path, content: content.clone() });
                }
                PatchOperation::DeleteFile { path } => {
                    let path =
                        vtcode_commons::paths::ensure_path_within_workspace_resolved(&root.join(path), root).await?;
                    self.require_existing_file(&path).await?;
                    prepared.push(PreparedPatchCredit::Removed { path });
                }
                PatchOperation::UpdateFile { path, chunks, new_path } => {
                    let source =
                        vtcode_commons::paths::ensure_path_within_workspace_resolved(&root.join(path), root).await?;
                    let destination = if let Some(destination) = new_path {
                        vtcode_commons::paths::ensure_path_within_workspace_resolved(&root.join(destination), root)
                            .await?
                    } else {
                        source.clone()
                    };
                    let before = match tokio::fs::read_to_string(&source).await {
                        Ok(before) => before,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            // Typed error: the canonical planner reports the
                            // same contract; pre-write inspection must match it.
                            return Err(PatchError::MissingFile { path: path.clone() }.into());
                        }
                        Err(error) => return Err(error.into()),
                    };
                    let inspection =
                        crate::tools::editing::patch::inspection_replacements(&source, &before, chunks, path).await?;
                    if !chunks.is_empty() {
                        self.require(&source, &before, &inspection.required_ranges)?;
                    }
                    let replacements = inspection.replacements;
                    let after =
                        crate::tools::editing::patch::render_patch_update_content(&source, &before, chunks, path)
                            .await?;
                    prepared.push(PreparedPatchCredit::Updated { source, destination, before, after, replacements });
                }
            }
        }
        Ok(prepared)
    }

    pub(crate) fn commit_patch(&self, prepared: Vec<PreparedPatchCredit>, support: String) {
        for credit in prepared {
            match credit {
                PreparedPatchCredit::Created { path, content } => self.record_created(&path, &content, support.clone()),
                PreparedPatchCredit::Removed { path } => {
                    let mut ledger = self.0.lock();
                    ledger.files.remove(&path);
                    ledger.order.retain(|entry| entry != &path);
                }
                PreparedPatchCredit::Updated { source, destination, before, after, replacements } => {
                    self.record_edit(&source, &before, &after, &replacements, support.clone());
                    if source != destination {
                        self.record_transfer(&source, &destination, &after, support.clone(), true);
                    }
                }
            }
        }
    }
}
