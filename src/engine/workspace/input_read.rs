//! Exact input proofs shared only by one synchronous editor read. The guard
//! owns the receipts; the Workspace's weak handle cannot extend their life.

use super::*;

#[derive(Default)]
pub(super) struct ReadMemo {
    revisions: HashMap<String, String>,
    invalidated: bool,
}

pub(crate) struct InputRead {
    memo: Arc<Mutex<ReadMemo>>,
}

impl InputRead {
    /// A final independent exact-byte check catches external writes during
    /// projection. It does not rebuild revisions or trust filesystem events.
    pub(crate) fn is_current(&self, workspace: &Workspace) -> bool {
        if !workspace.input_read.ptr_eq(&Arc::downgrade(&self.memo))
            || workspace.cancelled()
            || crate::engine::control::cancelled()
        {
            return false;
        }
        let memo = self.memo.lock().unwrap_or_else(|error| error.into_inner());
        !memo.invalidated
            && memo
                .revisions
                .keys()
                .all(|root| workspace.input_snapshot_is_current(root))
    }
}

impl Workspace {
    /// The caller must keep the scope inside one synchronous read and verify
    /// is_current before using its result. Snapshots never inherit this scope.
    /// Memory contains only roots/revision strings visited by that request;
    /// dropping the guard, including on unwind, releases all receipts.
    pub(crate) fn begin_input_read(&mut self) -> InputRead {
        let memo = Arc::new(Mutex::new(ReadMemo::default()));
        self.input_read = Arc::downgrade(&memo);
        InputRead { memo }
    }

    pub(super) fn read_revision(&self, root: &str) -> Option<String> {
        self.input_read
            .upgrade()?
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .revisions
            .get(root)
            .cloned()
    }

    pub(super) fn remember_read_revision(&self, root: String, revision: String) {
        if let Some(memo) = self.input_read.upgrade() {
            memo.lock()
                .unwrap_or_else(|error| error.into_inner())
                .revisions
                .insert(root, revision);
        }
    }

    pub(super) fn invalidate_read(&self, root: Option<&str>) {
        if let Some(memo) = self.input_read.upgrade() {
            let mut memo = memo.lock().unwrap_or_else(|error| error.into_inner());
            let changed = if let Some(root) = root {
                memo.revisions.remove(root).is_some()
            } else {
                let changed = !memo.revisions.is_empty();
                memo.revisions.clear();
                changed
            };
            memo.invalidated |= changed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> Workspace {
        Workspace::overlay_documents(&BTreeMap::from([(
            "root.mod".into(),
            "var y; model; y=1; end;".into(),
        )]))
    }

    #[test]
    fn input_read_receipts_expire_on_drop_unwind_and_snapshot() {
        // Guards a request proof escaping into a later request or a worker.
        let mut workspace = workspace();
        workspace.input_revision("root.mod").unwrap();
        let read = workspace.begin_input_read();
        INPUT_REVISION_CHECKS.with(|checks| checks.set(0));
        let before = workspace.input_revision("root.mod").unwrap();
        assert_eq!(workspace.input_revision("root.mod"), Some(before));
        assert_eq!(INPUT_REVISION_CHECKS.with(std::cell::Cell::get), 1);
        assert!(read.is_current(&workspace));
        let mut snapshot = workspace.snapshot_with_root_settings("root.mod");
        assert!(snapshot.input_read.upgrade().is_none());
        snapshot.input_revision("root.mod").unwrap();
        assert_eq!(INPUT_REVISION_CHECKS.with(std::cell::Cell::get), 2);
        drop(read);
        assert!(workspace.input_read.upgrade().is_none());
        workspace.input_revision("root.mod").unwrap();
        assert_eq!(INPUT_REVISION_CHECKS.with(std::cell::Cell::get), 3);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _read = workspace.begin_input_read();
            workspace.input_revision("root.mod").unwrap();
            panic!("test read unwind");
        }));
        assert!(result.is_err());
        assert!(workspace.input_read.upgrade().is_none());
    }

    #[test]
    fn input_read_rejects_mutation_cancellation_and_failed_proofs() {
        // Guards reuse after a new overlay/configuration or cancellation, and
        // treating an unavailable root as a valid cached input receipt.
        let mut workspace = workspace();
        let read = workspace.begin_input_read();
        assert!(workspace.input_revision("missing.mod").is_none());
        workspace.update_document("missing.mod", "var x;");
        assert!(workspace.input_revision("missing.mod").is_some());
        let before = workspace.input_revision("root.mod").unwrap();
        workspace.update_document("root.mod", "var y; model; y=2; end;");
        assert_ne!(workspace.input_revision("root.mod").unwrap(), before);
        assert!(!read.is_current(&workspace));
        drop(read);
        let read = workspace.begin_input_read();
        let before = workspace.input_revision("root.mod").unwrap();
        workspace.set_root_macro_work_limit("root.mod", MacroWorkLimit::new(1_000_000).unwrap());
        assert_ne!(workspace.input_revision("root.mod").unwrap(), before);
        assert!(!read.is_current(&workspace));
        drop(read);
        let read = workspace.begin_input_read();
        workspace.input_revision("root.mod").unwrap();
        let flag = Arc::new(AtomicBool::new(true));
        workspace.set_cancellation(flag);
        assert!(workspace.input_revision("root.mod").is_none());
        assert!(!read.is_current(&workspace));
        drop(read);
        assert!(workspace.input_read.upgrade().is_none());
    }

    #[test]
    fn input_read_final_guard_detects_disk_bytes_and_new_missing_candidates() {
        // Guards external writes during a request, including same-length
        // bytes and creation of a previously missing include candidate.
        let folder = std::env::temp_dir().join(format!(
            "dygnosis-read-proof-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&folder).unwrap();
        let root = path_key(&folder.join("root.mod"));
        let child = folder.join("child.inc");
        let mut workspace = Workspace::new();
        workspace.update_document(&root, "var y; model; y=\n@#include \"child.inc\"\n; end;");
        let read = workspace.begin_input_read();
        let missing = workspace.input_revision(&root).unwrap();
        assert!(read.is_current(&workspace));
        std::fs::write(&child, "1").unwrap();
        assert!(!read.is_current(&workspace));
        drop(read);
        let read = workspace.begin_input_read();
        let present = workspace.input_revision(&root).unwrap();
        assert_ne!(present, missing);
        assert!(read.is_current(&workspace));
        std::fs::write(&child, "2").unwrap();
        assert!(!read.is_current(&workspace));
        drop(read);
        assert_ne!(workspace.input_revision(&root).unwrap(), present);
        std::fs::remove_dir_all(&folder).unwrap();
    }

    #[test]
    fn superseded_read_scope_cannot_validate_a_newer_snapshot() {
        // Guards an outer or foreign scope validating its old facts against
        // a newer scope's input snapshot after the active receipt changed.
        let mut workspace = workspace();
        let outer = workspace.begin_input_read();
        workspace.input_revision("root.mod").unwrap();
        let inner = workspace.begin_input_read();
        workspace.update_document("root.mod", "var y; model; y=2; end;");
        workspace.input_revision("root.mod").unwrap();
        assert!(!outer.is_current(&workspace));
        assert!(inner.is_current(&workspace));
    }

    #[test]
    fn global_search_and_resource_changes_expire_read_receipts() {
        // Guards an active receipt bypassing changed search paths or newly
        // imposed source limits, rather than rebuilding its input proof.
        let mut workspace = workspace();
        let read = workspace.begin_input_read();
        let before = workspace.input_revision("root.mod").unwrap();
        workspace.add_search_path(PathBuf::from("new-search"));
        assert_ne!(workspace.input_revision("root.mod").unwrap(), before);
        assert!(!read.is_current(&workspace));
        drop(read);
        let read = workspace.begin_input_read();
        workspace.input_revision("root.mod").unwrap();
        workspace.set_source_limits(1, 1, 1);
        assert!(workspace.input_revision("root.mod").is_none());
        assert!(!read.is_current(&workspace));
    }
}
