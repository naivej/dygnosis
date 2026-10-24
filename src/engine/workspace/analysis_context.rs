//! Source guards for editor configuration actions, including stale selections.

use super::*;

impl Workspace {
    /// Recheck only the supplied source-inspection guards. These paths do not
    /// become executed inputs merely because a transport inspected them.
    pub(crate) fn source_guards_are_current(&self, guards: &[InputGuard]) -> bool {
        use sha2::{Digest, Sha256};
        let (file_limit, total_limit, file_count) =
            self.source_limits
                .unwrap_or((16 * 1024 * 1024, 16 * 1024 * 1024, 100_000));
        if guards.len() > file_count {
            return false;
        }
        let mut total = 0usize;
        guards.iter().all(|guard| {
            if self.cancelled() {
                return false;
            }
            let current = self.input_file(&guard.file);
            if current.exists != guard.exists
                || current.directory != guard.directory
                || current.identity != guard.identity
                || current.length != guard.length
            {
                return false;
            }
            let length = current
                .length
                .and_then(|length| usize::try_from(length).ok())
                .unwrap_or(0);
            if !current.directory {
                total = total.saturating_add(length);
                if length > file_limit || total > total_limit {
                    return false;
                }
            }
            if guard.byte_sha256.is_none() && guard.text_sha256.is_none() {
                // Lookup absence and directory identity need no file content.
                // A regular file without a content proof is never current.
                return !guard.exists || guard.directory;
            }
            let overlay = self.docs.get(&guard.file).filter(|doc| doc.overlay);
            let mut acquired = Vec::new();
            if current.bytes.is_none() && overlay.is_none() {
                // InputFile deliberately avoids retaining large byte buffers.
                // Recheck only this guarded file under the source allowance;
                // no parser, inventory or expansion runs during this proof.
                let Ok(mut file) = std::fs::File::open(&guard.file) else {
                    return false;
                };
                if acquired
                    .try_reserve_exact(length.saturating_add(1))
                    .is_err()
                {
                    return false;
                }
                let mut chunk = [0u8; 64 * 1024];
                loop {
                    if self.cancelled() {
                        return false;
                    }
                    let allowed = length
                        .saturating_add(1)
                        .saturating_sub(acquired.len())
                        .min(chunk.len());
                    if allowed == 0 {
                        return false;
                    }
                    let Ok(read) = file.read(&mut chunk[..allowed]) else {
                        return false;
                    };
                    if read == 0 {
                        break;
                    }
                    acquired.extend_from_slice(&chunk[..read]);
                    if acquired.len() > length {
                        return false;
                    }
                }
                if acquired.len() != length || self.input_file(&guard.file) != current {
                    return false;
                }
            }
            let bytes = current
                .bytes
                .as_deref()
                .or_else(|| overlay.map(|doc| doc.source.as_bytes()))
                .or_else(|| current.exists.then_some(acquired.as_slice()));
            let Some(bytes) = bytes else {
                return false;
            };
            if guard
                .byte_sha256
                .as_ref()
                .is_some_and(|expected| *expected != format!("{:x}", Sha256::digest(bytes)))
            {
                return false;
            }
            if let Some(expected) = &guard.text_sha256 {
                let digest = if let Some(doc) = overlay {
                    doc.source_digest()
                } else if std::str::from_utf8(bytes).is_ok() {
                    format!("{:x}", Sha256::digest(bytes))
                } else {
                    let mut digest = Sha256::new();
                    let mut encoded = [0u8; 2];
                    for (index, &byte) in bytes.iter().enumerate() {
                        if index % (64 * 1024) == 0 && self.cancelled() {
                            return false;
                        }
                        digest.update(char::from(byte).encode_utf8(&mut encoded).as_bytes());
                    }
                    format!("{:x}", digest.finalize())
                };
                if *expected != digest {
                    return false;
                }
            }
            !self.cancelled()
        })
    }

    /// Refresh known input facts without requiring an accepted configured model
    /// or building written inventories. This revision never proves semantics.
    pub(crate) fn source_context_revision(
        &mut self,
        root: &str,
    ) -> Result<String, ConfigurationError> {
        let key = self.ensure_loaded(root).ok_or_else(|| {
            ConfigurationError::new("ROOT_NOT_FOUND", "The model root is unavailable.")
        })?;
        // The normal input owner already captures executed/missing candidates
        // and validates reusable analysis. Use that complete revision directly;
        // the source-only path below is needed only for stale selections.
        if let Some(revision) = self.input_revision(root) {
            let mut hash = std::collections::hash_map::DefaultHasher::new();
            revision.hash(&mut hash);
            self.root_analyses
                .get(&key)
                .and_then(|analysis| analysis.selection.name.as_ref())
                .hash(&mut hash);
            return Ok(format!("{:016x}", hash.finish()));
        }
        let mut budget = self.macro_budget(&key);
        let mut files = std::collections::BTreeSet::from([key.clone()]);
        if let Some(inputs) = self.input_snapshots.get(&key) {
            for file in inputs.keys() {
                charge(&mut budget, file.len().saturating_add(1))?;
                files.insert(file.clone());
            }
        }
        if let Some(selection) = self.root_analyses.get(&key) {
            for selected in &selection.overrides {
                charge(&mut budget, selected.file.len().saturating_add(1))?;
                files.insert(selected.file.clone());
            }
        }
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        key.hash(&mut hash);
        self.overlay_only.hash(&mut hash);
        self.root_paths(&key).hash(&mut hash);
        self.root_macro_work_limit(&key).hash(&mut hash);
        let selection = self
            .root_analyses
            .get(&key)
            .map(|analysis| analysis.selection.clone())
            .unwrap_or_else(source_selection);
        selection.configuration_id.hash(&mut hash);
        selection.name.hash(&mut hash);
        let mut changed = false;
        for file in files {
            let length = self
                .docs
                .get(&file)
                .filter(|doc| doc.overlay)
                .map(|doc| doc.source.len())
                .or_else(|| {
                    (!self.overlay_only)
                        .then(|| {
                            std::fs::metadata(&file)
                                .ok()
                                .map(|metadata| metadata.len() as usize)
                        })
                        .flatten()
                })
                .unwrap_or(0);
            charge(
                &mut budget,
                length
                    .min(INPUT_FILE_BYTE_CAP)
                    .saturating_mul(2)
                    .saturating_add(file.len())
                    .saturating_add(1),
            )?;
            let state = self.input_file(&file);
            let differs = self
                .input_snapshots
                .get(&key)
                .and_then(|inputs| inputs.get(&file))
                .is_some_and(|previous| previous != &state);
            if !self.docs.get(&file).is_some_and(|doc| doc.overlay) {
                if let Some(bytes) = state.bytes.as_ref() {
                    charge(&mut budget, bytes.len().saturating_mul(2).saturating_add(1))?;
                    let text = String::from_utf8(bytes.clone()).unwrap_or_else(|error| {
                        error.into_bytes().into_iter().map(char::from).collect()
                    });
                    if self
                        .docs
                        .get(&file)
                        .is_none_or(|doc| doc.source.as_str() != text)
                    {
                        self.docs.insert(
                            file.clone(),
                            Arc::new(Doc {
                                source: Arc::new(text),
                                source_index: OnceLock::new(),
                                model: OnceLock::new(),
                                source_inventory: Mutex::new(Vec::new()),
                                macro_source: Mutex::new(None),
                                work_limit: self.root_macro_work_limit(&key),
                                overlay: false,
                                input_generation: 0,
                            }),
                        );
                        changed = true;
                    }
                } else if differs && !state.exists {
                    self.docs.remove(&file);
                    changed = true;
                } else if differs && state.exists && !state.directory {
                    // Large files have metadata guards in InputFile. Acquire
                    // their exact current bytes only when that guard changed.
                    let text = read_include_text(Path::new(&file), &mut budget)
                        .map_err(expression_error)?;
                    if let Some(text) = text {
                        self.docs.insert(
                            file.clone(),
                            Arc::new(Doc {
                                source: Arc::new(text),
                                source_index: OnceLock::new(),
                                model: OnceLock::new(),
                                source_inventory: Mutex::new(Vec::new()),
                                macro_source: Mutex::new(None),
                                work_limit: self.root_macro_work_limit(&key),
                                overlay: false,
                                input_generation: 0,
                            }),
                        );
                        changed = true;
                    }
                }
            }
            if differs {
                changed = true;
            }
            if let Some(previous) = self
                .input_snapshots
                .get_mut(&key)
                .and_then(|inputs| inputs.get_mut(&file))
            {
                *previous = state.clone();
            }
            file.hash(&mut hash);
            state.hash(&mut hash);
            if state.bytes.is_none()
                && let Some(doc) = self.docs.get(&file)
            {
                charge(&mut budget, doc.source.len().max(1))?;
                doc.source.hash(&mut hash);
            }
        }
        if changed {
            self.invalidate_root(&key);
        }
        charge(&mut budget, 1)?;
        Ok(format!("{:016x}", hash.finish()))
    }

    /// Explicit source inspection uses the existing bounded acquisition owner.
    /// It does not add an include edge or create a written-source parse.
    pub(crate) fn inspect_macro_source(
        &mut self,
        root: &str,
        file: &str,
        budget: &mut MacroBudget,
    ) -> Result<Option<Arc<MacroSource>>, ConfigurationError> {
        let file = self.include_key(&uri_to_path(file));
        let (text, _) = self.source_for_inspection(root, &file, budget)?;
        text.map(|_| {
            self.docs
                .get(&file)
                .expect("inspected source")
                .macro_source(budget)
                .map_err(expression_error)
        })
        .transpose()
    }
}

#[cfg(test)]
mod source_guard_tests {
    use super::*;

    #[test]
    fn large_second_side_guards_detect_same_length_disk_and_overlay_edits_without_parsing() {
        // Guards a >1MB helper reached only by side B being certified by its
        // unchanged length/path after its content changed.
        let directory = std::env::temp_dir().join(format!(
            "dygnosis-large-guard-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        // Normalize the existing directory before joining missing files: this
        // resolves macOS aliases and strips Windows's verbatim path prefix.
        let directory = PathBuf::from(path_key(&directory));
        let root = path_key(&directory.join("root.mod"));
        let helper = path_key(&directory.join("large.inc"));
        let text = "@#if ON\n@#include \"large.inc\"\n@#else\nvar y;model;y=0;end;\n@#endif\n";
        let large = format!(
            "// {}\nvar y;model;y=1;end;\n",
            "x".repeat(INPUT_FILE_BYTE_CAP + 1)
        );
        let changed = large.replacen("y=1", "y=2", 1);
        assert_eq!(large.len(), changed.len());
        std::fs::write(&root, text).unwrap();
        for overlay in [false, true] {
            std::fs::write(&helper, &large).unwrap();
            let make = |on: bool| {
                let mut workspace = if overlay {
                    Workspace::overlay_documents(&BTreeMap::from([
                        (root.clone(), text.to_owned()),
                        (helper.clone(), large.clone()),
                    ]))
                } else {
                    Workspace::new()
                };
                workspace.set_default_macro_work_limit(
                    MacroWorkLimit::new(MacroWorkLimit::MAX as u64).unwrap(),
                );
                workspace
                    .set_root_analysis_configuration(
                        &root,
                        AnalysisConfiguration {
                            defines: vec![InitialDefinition {
                                name: "ON".into(),
                                expression: on.to_string(),
                            }],
                            ..Default::default()
                        },
                    )
                    .unwrap();
                workspace.input_revision(&root).unwrap();
                workspace
            };
            let before = make(false);
            let mut after = make(true);
            assert!(
                !before
                    .input_guards(&root)
                    .iter()
                    .any(|guard| guard.file == helper)
            );
            let guards = after.input_guards(&root);
            let guard = guards.iter().find(|guard| guard.file == helper).unwrap();
            assert!(guard.text_sha256.is_some() && guard.byte_sha256.is_none());
            let parses =
                crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|builds| builds.get());
            assert!(after.source_guards_are_current(&guards));
            assert_eq!(
                crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|builds| builds.get()),
                parses
            );
            if overlay {
                after.update_document(&helper, changed.clone());
            } else {
                std::fs::write(&helper, &changed).unwrap();
            }
            let parses =
                crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|builds| builds.get());
            assert!(!after.source_guards_are_current(&guards));
            assert_eq!(
                crate::engine::language::parser::PARSE_EXPANDED_BUILDS.with(|builds| builds.get()),
                parses
            );
            let mut unproven = guard.clone();
            unproven.text_sha256 = None;
            assert!(!after.source_guards_are_current(&[unproven]));
        }
        let temporary = std::env::temp_dir().canonicalize().unwrap();
        let resolved = directory.canonicalize().unwrap();
        assert_eq!(resolved.parent(), Some(temporary.as_path()));
        std::fs::remove_dir_all(resolved).unwrap();
    }
}
