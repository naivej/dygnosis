//! Bounded written-declaration browsing. Inspected includes never become executed inputs.

use super::*;
use crate::engine::analysis::source_inventory::{
    SourceItem, SourceItemKind, SourceNameSite, SourcePathStep,
};
use crate::engine::language::lexer::Token;

const MAX_FILES: usize = 16;
const MAX_BYTES: usize = 256 * 1024;
const MAX_DEPTH: usize = 8;
const CACHE_FILES: usize = 8;
const CACHE_BYTES: usize = 8 * 1024 * 1024;

#[derive(Clone)]
pub(super) struct CachedSource {
    file: String,
    doc: Arc<Doc>,
    guard: InputGuard,
    bytes: usize,
    inventories: Vec<(MacroWorkLimit, std::sync::Weak<SourceInventory>)>,
}

pub(crate) struct WrittenDeclaration {
    pub item: SourceItem,
    pub file: String,
    pub line: u32,
}

#[derive(Default)]
pub(crate) struct DeclarationLookup {
    pub declaration: Option<WrittenDeclaration>,
    pub cause: Option<String>,
    pub guards: Vec<InputGuard>,
    pub sources: Vec<DeclarationSource>,
    pub checked_includes: Vec<(String, Span)>,
    pub resolutions: Vec<(String, String, Option<String>)>,
}

pub(crate) struct DeclarationSource {
    pub file: String,
    pub text: Arc<String>,
    pub revision: String,
    pub complete: bool,
    pub cause: Option<String>,
    pub inventory: Option<Arc<SourceInventory>>,
}

struct Search<'a> {
    workspace: &'a mut Workspace,
    root: &'a str,
    lookup: Arc<SourceLookup>,
    budget: MacroBudget,
    limit: MacroWorkLimit,
    name: String,
    role: Option<SourceNameSite>,
    local_scope: Option<String>,
    files: HashMap<String, (Arc<Doc>, Arc<SourceInventory>)>,
    stack: Vec<String>,
    bytes: usize,
    guards: Vec<InputGuard>,
    edges: Vec<(String, String, String)>,
    candidate: Option<WrittenDeclaration>,
    written_search_path: bool,
    inspected: HashMap<String, DeclarationSource>,
    checked_includes: Vec<(String, Span)>,
    resolutions: Vec<(String, String, Option<String>)>,
    incoming_site: Option<(String, u32)>,
    subsample_epoch: bool,
}

fn compatible(path: &[SourcePathStep], current: &[SourcePathStep]) -> bool {
    !path.iter().any(|candidate| {
        current
            .iter()
            .any(|step| step.opener == candidate.opener && step.branch != candidate.branch)
    })
}

impl Workspace {
    pub(crate) fn written_declaration_at_use(
        &mut self,
        root: &str,
        document: &str,
        file: &SourceFileStructure,
        item: &SourceItem,
        byte: u32,
    ) -> DeclarationLookup {
        let Some((selection, role)) = file.inventory.name_use(item, byte) else {
            return DeclarationLookup::default();
        };
        let root_key = if self.overlay_only {
            root.to_owned()
        } else {
            normalize_uri(root)
        };
        let document = if self.overlay_only {
            document.to_owned()
        } else {
            normalize_uri(document)
        };
        if let Some(site) = role.as_ref().filter(|site| site.declaration)
            && let Some(item) = file.inventory.named_declaration(&file.text, site)
        {
            let line = self
                .docs
                .get(&document)
                .map(|doc| {
                    doc.source_index()
                        .position(&file.text, site.span.start)
                        .line
                        + 1
                })
                .unwrap_or(1);
            return DeclarationLookup {
                declaration: Some(WrittenDeclaration {
                    item,
                    file: document,
                    line,
                }),
                ..DeclarationLookup::default()
            };
        }
        let limit = self.root_macro_work_limit(root);
        let mut search = Search {
            lookup: file.lookup.as_ref().cloned().unwrap_or_else(|| {
                Arc::new(SourceLookup {
                    known: HashSet::new(),
                    aliases: HashMap::new(),
                })
            }),
            budget: self.macro_budget(&root_key),
            limit,
            name: file.text[selection.start as usize..selection.end as usize]
                .trim_matches(['\'', '"'])
                .to_owned(),
            role,
            local_scope: (item.kind == SourceItemKind::Equation)
                .then(|| item.source_namespace.clone())
                .flatten(),
            files: HashMap::new(),
            stack: vec![document.clone()],
            bytes: 0,
            guards: Vec::new(),
            edges: Vec::new(),
            candidate: None,
            written_search_path: false,
            inspected: HashMap::new(),
            checked_includes: Vec::new(),
            resolutions: Vec::new(),
            incoming_site: None,
            subsample_epoch: false,
            workspace: self,
            root: &root_key,
        };
        let mut parent_cause = None;
        let parent = if document != root_key {
            match search.acquire(&root_key) {
                Ok((doc, inventory)) => {
                    let mut routes = Vec::new();
                    for row in &inventory.items {
                        if let Err(cause) = search.spend(1) {
                            parent_cause = Some(cause);
                            routes.clear();
                            break;
                        }
                        if row.kind != SourceItemKind::MacroDirective
                            || row.written_kind.as_deref() != Some("include")
                        {
                            continue;
                        }
                        let directive =
                            &doc.source[row.selection.start as usize..row.selection.end as usize];
                        if let Err(cause) = search.spend(
                            directive.len()
                                + root_key.len()
                                + search
                                    .workspace
                                    .root_paths(&root_key)
                                    .iter()
                                    .map(|path| path.as_os_str().len())
                                    .sum::<usize>(),
                        ) {
                            parent_cause = Some(cause);
                            routes.clear();
                            break;
                        }
                        if let Some(literal) =
                            crate::engine::language::parser::literal_include_filename(directive)
                            && search.resolve_recorded(&root_key, &literal).as_deref()
                                == Some(document.as_str())
                        {
                            routes.push((row.selection.start, row.path.clone(), literal));
                        }
                    }
                    if let [route] = routes.as_slice() {
                        let final_table = search
                            .role
                            .as_ref()
                            .is_some_and(SourceNameSite::final_table);
                        if final_table {
                            search.incoming_site = Some((root_key.clone(), route.0));
                        }
                        let result = search.visit(
                            &root_key,
                            &doc.source,
                            &inventory,
                            if final_table { u32::MAX } else { route.0 },
                            &route.1,
                        );
                        search
                            .edges
                            .push((root_key.clone(), route.2.clone(), document.clone()));
                        result
                    } else {
                        parent_cause.get_or_insert_with(|| "Parent-file declarations were not checked because the literal include site could not be identified uniquely.".into());
                        Ok(())
                    }
                }
                Err(cause) => {
                    parent_cause = Some(format!(
                        "Parent-file declarations were not checked: {cause}"
                    ));
                    Ok(())
                }
            }
        } else {
            Ok(())
        };
        if let Err(cause) = parent {
            parent_cause = Some(format!(
                "Parent-file declarations were not checked: {cause}"
            ));
            search.candidate = None;
        }
        let result = search.visit(
            &document,
            &file.text,
            &file.inventory,
            if search
                .role
                .as_ref()
                .is_some_and(SourceNameSite::final_table)
            {
                u32::MAX
            } else {
                selection.start
            },
            file.inventory.macro_path(byte),
        );
        let mut failure = result.err();
        if search.workspace.cancelled() {
            failure = Some("Declaration lookup was cancelled.".into());
        } else if search
            .resolutions
            .iter()
            .any(|(including, literal, target)| search.resolve(including, literal) != *target)
            || search.edges.iter().any(|(including, literal, target)| {
                search.resolve(including, literal).as_deref() != Some(target)
            })
        {
            failure =
                Some("An include target changed while checking declarations. Try again.".into());
        } else if !search.workspace.source_guards_are_current(&search.guards) {
            failure =
                Some("An included source changed while checking declarations. Try again.".into());
        }
        if failure.is_some() {
            search.candidate = None;
        }
        let mut sources: Vec<_> = search.inspected.into_values().collect();
        sources.sort_by(|left, right| left.file.cmp(&right.file));
        DeclarationLookup {
            declaration: search.candidate,
            cause: failure.or(parent_cause),
            guards: search.guards,
            sources,
            checked_includes: search.checked_includes,
            resolutions: search.resolutions,
        }
    }
}

impl Search<'_> {
    fn spend(&mut self, work: usize) -> Result<(), String> {
        if self.workspace.cancelled() {
            return Err("Declaration lookup was cancelled.".into());
        }
        self.budget
            .spend_work(work.max(1))
            .map_err(|_| "Declaration lookup reached the macro work allowance.".into())
    }

    fn resolve(&self, including: &str, literal: &str) -> Option<String> {
        // A dormant branch cannot borrow @#includepath from selected execution.
        let paths = self.workspace.root_paths(self.root);
        self.resolve_with_paths(including, literal, paths)
    }

    fn resolve_recorded(&mut self, including: &str, literal: &str) -> Option<String> {
        let target = self.resolve(including, literal);
        for key in self.workspace.source_candidate_keys(
            self.root,
            including,
            literal,
            self.workspace.root_paths(self.root),
        ) {
            if target.as_ref() == Some(&key) {
                break;
            }
            let input = self.workspace.input_file(&key);
            if !input.exists || input.directory {
                self.guards.push(InputGuard {
                    file: key,
                    exists: input.exists,
                    directory: input.directory,
                    identity: input.identity,
                    length: input.length,
                    text_sha256: None,
                    byte_sha256: None,
                });
            }
        }
        self.resolutions
            .push((including.to_owned(), literal.to_owned(), target.clone()));
        target
    }

    fn resolve_with_paths(
        &self,
        including: &str,
        literal: &str,
        paths: &[PathBuf],
    ) -> Option<String> {
        if self.workspace.overlay_only {
            return self
                .workspace
                .overlay_candidates(including, literal, paths)
                .into_iter()
                .find_map(|candidate| self.lookup.aliases.get(&candidate).cloned().flatten());
        }
        if self.workspace.virtual_roots.contains(self.root) {
            return None;
        }
        let resolved =
            resolve_scoped_include_path(literal, including, paths, Some(&self.lookup.known));
        resolved.map(|path| self.workspace.include_key(&path))
    }

    fn acquire(&mut self, file: &str) -> Result<(Arc<Doc>, Arc<SourceInventory>), String> {
        if let Some(found) = self.files.get(file) {
            return Ok(found.clone());
        }
        if self.files.len() >= MAX_FILES {
            return Err(format!(
                "Declaration lookup reached its {MAX_FILES}-file limit."
            ));
        }
        let length = self
            .workspace
            .docs
            .get(file)
            .filter(|doc| doc.overlay)
            .map(|doc| doc.source.len() as u64)
            .or_else(|| {
                (!self.workspace.overlay_only)
                    .then(|| std::fs::metadata(file).ok().map(|meta| meta.len()))
                    .flatten()
            })
            .unwrap_or(0);
        if length > MAX_BYTES.saturating_sub(self.bytes) as u64 {
            return Err(format!(
                "Declaration lookup reached its {MAX_BYTES}-byte source limit."
            ));
        }
        let cached = self.workspace.source_declarations.iter().position(|entry| {
            entry.file == file
                && self
                    .workspace
                    .source_guards_are_current(std::slice::from_ref(&entry.guard))
        });
        let (doc, mut guard, cached_bytes) = if let Some(index) = cached {
            let entry = self.workspace.source_declarations.remove(index);
            let unchanged = {
                let inventories = entry
                    .doc
                    .source_inventory
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                inventories.len() == entry.inventories.len()
                    && inventories.iter().zip(&entry.inventories).all(
                        |((limit, inventory), (old_limit, old))| {
                            limit == old_limit
                                && old
                                    .upgrade()
                                    .is_some_and(|old| Arc::ptr_eq(&old, inventory))
                        },
                    )
            };
            (entry.doc, entry.guard, unchanged.then_some(entry.bytes))
        } else {
            // Only acquisition runs in this view. It contains no effective model,
            // executed include records, dependencies or semantic owner relations.
            let mut snapshot = Workspace {
                overlay_only: self.workspace.overlay_only,
                default_macro_work_limit: self.limit,
                cancellation: self.workspace.cancellation.clone(),
                source_limits: Some(self.workspace.source_limits.map_or(
                    (MAX_BYTES, MAX_BYTES, MAX_FILES),
                    |(file, total, count)| {
                        (
                            file.min(MAX_BYTES),
                            total.min(MAX_BYTES),
                            count.min(MAX_FILES),
                        )
                    },
                )),
                ..Workspace::default()
            };
            if let Some(doc) = self.workspace.docs.get(file) {
                snapshot.docs.insert(file.to_owned(), Arc::clone(doc));
            }
            let (_, guard) = snapshot
                .source_for_inspection(self.root, file, &mut self.budget)
                .map_err(|error| error.message)?;
            let doc = snapshot
                .docs
                .remove(file)
                .ok_or_else(|| format!("Could not read include {file}."))?;
            (doc, guard, None)
        };
        self.bytes += doc.source.len();
        guard.text_sha256 = Some(doc.source_digest());
        self.guards.push(guard.clone());
        self.inspected.insert(
            file.to_owned(),
            DeclarationSource {
                file: file.to_owned(),
                text: Arc::clone(&doc.source),
                revision: doc.source_digest(),
                complete: false,
                cause: Some("Written source inspection did not finish.".into()),
                inventory: None,
            },
        );
        if self.bytes > MAX_BYTES {
            return Err(format!(
                "Declaration lookup reached its {MAX_BYTES}-byte source limit."
            ));
        }
        let macro_source = doc
            .macro_source(&mut self.budget)
            .map_err(|error| match error {
                MacroEvalError::Limit(limit) => {
                    format!("Include macro syntax reached the {limit} limit.")
                }
                error => error
                    .diagnostic()
                    .map(|(_, message)| message)
                    .unwrap_or_else(|| "Include macro syntax is unavailable.".into()),
            })?;
        let cached_inventory = doc
            .source_inventory
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .iter()
            .find(|(_, inventory)| {
                inventory.structure_complete() && inventory.work_used <= self.limit.get()
            })
            .map(|(_, inventory)| Arc::clone(inventory));
        let inventory_reused = cached_inventory.is_some();
        let inventory = if let Some(inventory) = cached_inventory {
            inventory
        } else {
            let build_limit =
                MacroWorkLimit::new(self.budget.remaining_work().min(self.limit.get()) as u64)
                    .map_err(|_| "Declaration lookup reached the macro work allowance.")?;
            let inventory = doc
                .source_inventory_with_limit(build_limit, self.workspace.cancellation.clone())
                .ok_or("Declaration lookup was cancelled.")?;
            self.spend(inventory.work_used)?;
            inventory
        };
        if !inventory.structure_complete() || !inventory.targets_complete() {
            if let Some(source) = self.inspected.get_mut(file) {
                source.cause = inventory
                    .structure_cause()
                    .or(inventory.target_coverage_cause.as_deref())
                    .map(str::to_owned);
            }
            return Err(inventory
                .structure_cause()
                .or(inventory.target_coverage_cause.as_deref())
                .unwrap_or("Written include structure is incomplete.")
                .into());
        }
        if let Some(source) = self.inspected.get_mut(file) {
            source.complete = true;
            source.cause = None;
            source.inventory = Some(Arc::clone(&inventory));
        }
        // Account for both allowance variants and shared macro payload. The
        // private source-only Doc never evaluates its standalone model.
        if !self.workspace.docs.contains_key(file) {
            debug_assert!(doc.model.get().is_none());
            let bytes = cached_bytes
                .filter(|_| inventory_reused)
                .unwrap_or_else(|| {
                    std::mem::size_of::<Doc>()
                        + std::mem::size_of::<CachedSource>()
                        + std::mem::size_of::<MacroSource>()
                        + 512
                        + file.len() * 2
                        + doc.source.capacity()
                        + macro_source.normalized.capacity()
                        + doc.source.len() * 4
                        + macro_source.source_revision.capacity()
                        + macro_source
                            .syntax_refusal
                            .as_ref()
                            .map_or(0, |refusal| refusal.message.capacity())
                        + macro_source.tokens.len() * std::mem::size_of::<Token>()
                        + macro_source
                            .sites
                            .iter()
                            .map(source_structure::site_bytes)
                            .sum::<usize>()
                        + doc
                            .source_inventory
                            .lock()
                            .unwrap_or_else(|error| error.into_inner())
                            .iter()
                            .map(|(_, inventory)| inventory.retained_bytes())
                            .sum::<usize>()
                });
            self.workspace
                .source_declarations
                .retain(|entry| entry.file != file);
            if bytes <= CACHE_BYTES {
                while self.workspace.source_declarations.len() >= CACHE_FILES
                    || self
                        .workspace
                        .source_declarations
                        .iter()
                        .map(|entry| entry.bytes)
                        .sum::<usize>()
                        > CACHE_BYTES - bytes
                {
                    self.workspace.source_declarations.remove(0);
                }
                self.workspace.source_declarations.push(CachedSource {
                    file: file.to_owned(),
                    doc: Arc::clone(&doc),
                    guard,
                    bytes,
                    inventories: doc
                        .source_inventory
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .iter()
                        .map(|(limit, inventory)| (*limit, Arc::downgrade(inventory)))
                        .collect(),
                });
            }
        }
        self.files
            .insert(file.to_owned(), (Arc::clone(&doc), Arc::clone(&inventory)));
        Ok((doc, inventory))
    }

    fn visit(
        &mut self,
        file: &str,
        text: &str,
        inventory: &SourceInventory,
        before: u32,
        path: &[SourcePathStep],
    ) -> Result<(), String> {
        if let Some(role) = self.role.clone().or_else(|| {
            self.local_scope.as_ref().map(|scope| SourceNameSite {
                span: Span::new(0, 0),
                kind: "model_local_variable",
                declaration: false,
                namespace: Some(scope.clone()),
                owner: None,
            })
        }) {
            for site in &inventory.named_sites {
                self.spend(1)?;
                if role.kind == "named_subsample"
                    && site.kind == "subsample_reset"
                    && site.namespace == role.namespace
                    && site.span.start < before
                    && compatible(inventory.macro_path(site.span.start), path)
                {
                    if !path.starts_with(inventory.macro_path(site.span.start)) {
                        return Err(
                            "A subsample declaration depends on another macro branch.".into()
                        );
                    }
                    if self.subsample_epoch {
                        return Err("Multiple written subsample declarations for the same owner prevent selecting a declaration.".into());
                    }
                    self.subsample_epoch = true;
                }
                if role.kind == "named_subsample"
                    && site.kind == "subsample_copy"
                    && site.namespace == role.namespace
                    && site.span.start < before
                    && compatible(inventory.macro_path(site.span.start), path)
                {
                    return Err("Written subsample copies are not resolved for this name.".into());
                }
                if !site.declaration
                    || site.span.start >= before
                    || !role.matches(site)
                    || text
                        .get(site.span.start as usize..site.span.end as usize)
                        .map(|name| name.trim_matches(['\'', '"']))
                        != Some(self.name.as_str())
                {
                    continue;
                }
                let Some(row) = inventory.named_declaration(text, site) else {
                    continue;
                };
                if !compatible(&row.path, path) {
                    continue;
                }
                if !path.starts_with(&row.path) {
                    return Err(format!(
                        "A matching declaration in {file} depends on another macro branch."
                    ));
                }
                if row.local_definition.is_some()
                    && self
                        .candidate
                        .as_ref()
                        .is_some_and(|candidate| candidate.item.local_definition.is_none())
                {
                    self.candidate = None;
                }
                self.select(file, text, row)?;
            }
        }
        for row in &inventory.items {
            self.spend(1)?;
            if row.selection.start >= before || !compatible(&row.path, path) {
                continue;
            }
            if (self.role.is_none()
                || self
                    .role
                    .as_ref()
                    .is_some_and(|role| role.kind == "callable"))
                && row.name.as_deref() == Some(self.name.as_str())
            {
                if row.local_definition.is_some() {
                    continue;
                }
                if row.kind == SourceItemKind::Declaration && row.span == row.selection {
                    if self
                        .candidate
                        .as_ref()
                        .is_some_and(|candidate| candidate.item.local_definition.is_some())
                    {
                        continue;
                    }
                    if !path.starts_with(&row.path) {
                        return Err(format!(
                            "A matching declaration in {file} depends on another macro branch."
                        ));
                    }
                    if !matches!(
                        row.written_kind.as_deref(),
                        Some(
                            "var"
                                | "varexo"
                                | "varexo_det"
                                | "parameters"
                                | "trend_var"
                                | "log_trend_var"
                        )
                    ) {
                        if self.role.is_none() {
                            return Err(
                                "This kind of matching declaration is not supported by source lookup."
                                    .into(),
                            );
                        }
                        continue;
                    }
                    self.select(file, text, row.clone())?;
                }
            }
            if row.kind != SourceItemKind::MacroDirective {
                continue;
            }
            if row.written_kind.as_deref() == Some("includepath") {
                self.written_search_path = true;
                continue;
            }
            if row.written_kind.as_deref() != Some("include") {
                continue;
            }
            if self
                .incoming_site
                .as_ref()
                .is_some_and(|(parent, start)| parent == file && *start == row.selection.start)
            {
                continue;
            }
            if !path.starts_with(&row.path) {
                return Err("Cannot check an include behind another macro branch.".into());
            }
            let directive = &text[row.selection.start as usize..row.selection.end as usize];
            let literal = crate::engine::language::parser::literal_include_filename(directive)
                .ok_or("Cannot check a computed include path.")?;
            self.spend(
                file.len()
                    + literal.len()
                    + self
                        .workspace
                        .root_paths(self.root)
                        .iter()
                        .map(|path| path.as_os_str().len())
                        .sum::<usize>(),
            )?;
            let target = self.resolve_recorded(file,&literal).ok_or_else(|| format!("Could not check include {literal}: the file is unavailable in the configured include paths."))?;
            if self.written_search_path
                && self.resolve_with_paths(file, &literal, &[]).as_deref() != Some(target.as_str())
            {
                return Err(format!(
                    "Could not check include {literal}: written include search paths require macro execution."
                ));
            }
            if self.stack.contains(&target) {
                return Err(format!(
                    "Could not check include {literal}: the literal include chain contains a cycle."
                ));
            }
            if self.stack.len() >= MAX_DEPTH {
                return Err(format!(
                    "Declaration lookup reached its {MAX_DEPTH}-level include limit."
                ));
            }
            self.edges
                .push((file.to_owned(), literal.clone(), target.clone()));
            let (doc, child) = self
                .acquire(&target)
                .map_err(|cause| format!("Could not check include {literal}: {cause}"))?;
            self.stack.push(target.clone());
            let result = self.visit(&target, &doc.source, &child, u32::MAX, &[]);
            self.stack.pop();
            result?;
            self.checked_includes.push((file.to_owned(), row.selection));
        }
        Ok(())
    }

    fn select(&mut self, file: &str, text: &str, row: SourceItem) -> Result<(), String> {
        if self.candidate.as_ref().is_some_and(|candidate| {
            candidate.file != file || candidate.item.selection != row.selection
        }) {
            return Err("More than one matching written declaration was found.".into());
        }
        if self.candidate.is_none() {
            let line = self
                .files
                .get(file)
                .map(|(doc, _)| doc)
                .or_else(|| self.workspace.docs.get(file))
                .map(|doc| doc.source_index().position(text, row.selection.start).line + 1)
                .unwrap_or(1);
            self.candidate = Some(WrittenDeclaration {
                item: row,
                file: file.to_owned(),
                line,
            });
        }
        Ok(())
    }
}
