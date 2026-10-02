//! LSP configuration snapshots. Presentation never changes shared analysis.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;
use tower_lsp::lsp_types::{Url, WorkspaceFolder};

use crate::format::parse_format_indent;
use crate::include_resolver::{normalize_uri, path_key};

pub const CONFIGURATION_SCHEMA_VERSION: u32 = 1;
pub const MODEL_INFO_SCHEMA_VERSION: u32 = 1;
const OUTLINE_SECTIONS: &[&str] = &[
    "declarations",
    "blocks",
    "commands",
    "dimensions",
    "equations",
];

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NameDetails {
    pub long_name: bool,
    pub tex: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct OutlinePreferences {
    pub sections: Vec<String>,
    pub equation_numbers: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct PresentationSettings {
    pub name_details: NameDetails,
    pub outline: OutlinePreferences,
    pub parameter_value_hints: bool,
}

impl Default for PresentationSettings {
    fn default() -> Self {
        Self {
            name_details: NameDetails {
                long_name: true,
                tex: true,
            },
            outline: OutlinePreferences {
                sections: OUTLINE_SECTIONS.iter().map(|s| (*s).to_owned()).collect(),
                equation_numbers: true,
            },
            parameter_value_hints: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ResourceSettings {
    pub search_paths: Vec<PathBuf>,
    pub format_indent_unit: String,
    pub presentation: PresentationSettings,
    pub project_exclude_paths: Vec<String>,
}

impl Default for ResourceSettings {
    fn default() -> Self {
        Self {
            search_paths: Vec::new(),
            format_indent_unit: "\t".to_owned(),
            presentation: PresentationSettings::default(),
            project_exclude_paths: Vec::new(),
        }
    }
}

#[derive(Clone)]
pub(crate) struct SettingsStore {
    pub folders: Vec<WorkspaceFolder>,
    loose: ResourceSettings,
    folder_settings: HashMap<String, ResourceSettings>,
    legacy_root_paths: HashMap<String, Vec<PathBuf>>,
    pub project_diagnostics: bool,
}

impl Default for SettingsStore {
    fn default() -> Self {
        Self {
            folders: Vec::new(),
            loose: ResourceSettings::default(),
            folder_settings: HashMap::new(),
            legacy_root_paths: HashMap::new(),
            project_diagnostics: true,
        }
    }
}

impl SettingsStore {
    pub fn set_folders(&mut self, folders: Vec<WorkspaceFolder>) {
        self.folders = folders;
        self.folders
            .sort_by(|a, b| a.uri.as_str().cmp(b.uri.as_str()));
        self.folders.dedup_by(|a, b| a.uri == b.uri);
    }

    pub fn change_folders(&mut self, added: Vec<WorkspaceFolder>, removed: Vec<WorkspaceFolder>) {
        for folder in removed {
            self.folders.retain(|item| item.uri != folder.uri);
            self.folder_settings
                .remove(&normalize_uri(folder.uri.as_str()));
        }
        let mut folders = std::mem::take(&mut self.folders);
        folders.extend(added);
        self.set_folders(folders);
    }

    /// Complete snapshots replace all values; the legacy route remains partial.
    pub fn apply(&mut self, value: &Value) -> Vec<String> {
        let value = value.get("dynare").unwrap_or(value);
        let Some(object) = value.as_object() else {
            return Vec::new();
        };
        let mut explanations = Vec::new();
        if let Some(snapshot) = object.get("configuration") {
            if snapshot.get("schemaVersion").and_then(Value::as_u64)
                != Some(u64::from(CONFIGURATION_SCHEMA_VERSION))
            {
                return vec!["Unsupported dynare.configuration schemaVersion; keep the current settings and use schemaVersion 1.".to_owned()];
            }
            let mut loose = ResourceSettings::default();
            read_settings(
                snapshot.get("loose").unwrap_or(&Value::Null),
                &mut loose,
                &mut explanations,
            );
            let mut settings = HashMap::new();
            if let Some(folders) = snapshot.get("folders").and_then(Value::as_array) {
                for folder in folders {
                    let Some(uri) = folder
                        .get("uri")
                        .and_then(Value::as_str)
                        .and_then(|s| Url::parse(s).ok())
                        .filter(|u| u.scheme() == "file")
                    else {
                        explanations.push(
                            "Invalid dynare.configuration folder URI; ignore this folder entry."
                                .to_owned(),
                        );
                        continue;
                    };
                    let mut resource = ResourceSettings::default();
                    read_settings(
                        folder.get("settings").unwrap_or(&Value::Null),
                        &mut resource,
                        &mut explanations,
                    );
                    settings.insert(normalize_uri(uri.as_str()), resource);
                }
            } else {
                explanations.push(
                    "Invalid dynare.configuration folders; use loose-file defaults.".to_owned(),
                );
            }
            self.loose = loose;
            self.project_diagnostics = snapshot
                .get("loose")
                .and_then(|v| v.get("dynare").or(Some(v)))
                .and_then(|v| v.get("projectDiagnostics"))
                .map(|v| {
                    v.as_bool().unwrap_or_else(|| {
                        explanations
                            .push("Invalid dynare.projectDiagnostics; use true.".to_owned());
                        true
                    })
                })
                .unwrap_or(true);
            self.folder_settings = settings;
            self.legacy_root_paths.clear();
        } else {
            if let Some(value) = object.get("projectDiagnostics") {
                self.project_diagnostics = value.as_bool().unwrap_or_else(|| {
                    explanations.push("Invalid dynare.projectDiagnostics; use true.".to_owned());
                    true
                });
            }
            read_settings(value, &mut self.loose, &mut explanations);
            // The new window switch alone does not replace resource scopes.
            if object.len() != 1 || !object.contains_key("projectDiagnostics") {
                self.folder_settings.clear();
            }
            if let Some(raw) = object.get("searchPathsByRoot") {
                self.legacy_root_paths.clear();
                if let Some(roots) = raw.as_object() {
                    for (root, paths) in roots {
                        self.legacy_root_paths
                            .insert(normalize_uri(root), read_paths(paths, &mut explanations));
                    }
                } else {
                    explanations
                        .push("Invalid dynare.searchPathsByRoot; use searchPaths only.".to_owned());
                }
            }
        }
        explanations
    }

    pub fn resolve(&self, uri: &Url) -> ResourceSettings {
        let document = uri.to_file_path().ok().map(|p| PathBuf::from(path_key(&p)));
        let folder = document.as_ref().and_then(|document| {
            self.folders
                .iter()
                .filter_map(|folder| {
                    let path = folder.uri.to_file_path().ok()?;
                    let path = PathBuf::from(path_key(&path));
                    document.starts_with(&path).then_some((path, folder))
                })
                .max_by_key(|(path, _)| path.components().count())
        });
        let mut settings = folder
            .as_ref()
            .and_then(|(_, folder)| {
                self.folder_settings
                    .get(&normalize_uri(folder.uri.as_str()))
            })
            .unwrap_or(&self.loose)
            .clone();
        let root_key = normalize_uri(uri.as_str());
        let extra = self.legacy_root_paths.get(&root_key).or_else(|| {
            folder.as_ref().and_then(|(_, folder)| {
                self.legacy_root_paths
                    .get(&normalize_uri(folder.uri.as_str()))
            })
        });
        if let Some(extra) = extra {
            settings.search_paths.extend(extra.iter().cloned());
        }
        let base = folder
            .as_ref()
            .map(|(path, _)| path.as_path())
            .or_else(|| document.as_deref().and_then(Path::parent));
        let mut paths = Vec::new();
        for path in settings.search_paths {
            let resolved = if path.is_absolute() {
                path
            } else if let Some(base) = base {
                base.join(path)
            } else {
                continue;
            };
            let resolved = PathBuf::from(path_key(&resolved));
            if !paths.contains(&resolved) {
                paths.push(resolved);
            }
        }
        settings.search_paths = paths;
        settings
    }
}

fn read_settings(value: &Value, settings: &mut ResourceSettings, explanations: &mut Vec<String>) {
    let value = value.get("dynare").unwrap_or(value);
    let Some(object) = value.as_object() else {
        return;
    };
    if let Some(raw) = object.get("projectExcludePaths") {
        settings.project_exclude_paths.clear();
        if let Some(items) = raw.as_array() {
            for item in items {
                if let Some(pattern) = item.as_str().filter(|s| !s.trim().is_empty()) {
                    if !settings.project_exclude_paths.iter().any(|s| s == pattern) {
                        settings.project_exclude_paths.push(pattern.to_owned());
                    }
                } else {
                    explanations.push(
                        "Invalid dynare.projectExcludePaths entry; ignore this entry.".to_owned(),
                    );
                }
            }
        } else {
            explanations.push("Invalid dynare.projectExcludePaths; use no exclusions.".to_owned());
        }
    }
    if let Some(raw) = object.get("searchPaths") {
        settings.search_paths = read_paths(raw, explanations);
    }
    if let Some(raw) = object.get("formatIndent") {
        settings.format_indent_unit = parse_format_indent(raw).unwrap_or_else(|| {
            explanations.push("Invalid dynare.formatIndent; use a tab.".to_owned());
            "\t".to_owned()
        });
    }
    if let Some(details) = object.get("nameDetails") {
        if !details.is_object() {
            explanations.push("Invalid dynare.nameDetails; show long names and TeX.".to_owned());
            settings.presentation.name_details = PresentationSettings::default().name_details;
        }
        read_bool(
            details,
            "longName",
            &mut settings.presentation.name_details.long_name,
            explanations,
        );
        read_bool(
            details,
            "tex",
            &mut settings.presentation.name_details.tex,
            explanations,
        );
    }
    if let Some(outline) = object.get("outline") {
        if !outline.is_object() {
            explanations
                .push("Invalid dynare.outline; use all sections and equation numbers.".to_owned());
            settings.presentation.outline = PresentationSettings::default().outline;
        }
        read_bool(
            outline,
            "equationNumbers",
            &mut settings.presentation.outline.equation_numbers,
            explanations,
        );
        if let Some(raw) = outline.get("sections") {
            let mut sections = Vec::new();
            if let Some(items) = raw.as_array() {
                for item in items {
                    if let Some(section) = item.as_str().filter(|s| OUTLINE_SECTIONS.contains(s)) {
                        if !sections.iter().any(|s| s == section) {
                            sections.push(section.to_owned());
                        }
                    } else {
                        explanations.push(
                            "Invalid dynare.outline.sections entry; ignore this entry.".to_owned(),
                        );
                    }
                }
            } else {
                explanations.push("Invalid dynare.outline.sections; use all sections.".to_owned());
                sections = PresentationSettings::default().outline.sections;
            }
            settings.presentation.outline.sections = sections;
        }
    }
    read_bool(
        value,
        "parameterValueHints",
        &mut settings.presentation.parameter_value_hints,
        explanations,
    );
}

fn read_bool(value: &Value, key: &str, setting: &mut bool, explanations: &mut Vec<String>) {
    if let Some(raw) = value.get(key) {
        *setting = raw.as_bool().unwrap_or_else(|| {
            explanations.push(format!("Invalid dynare {key}; use true."));
            true
        });
    }
}

fn read_paths(raw: &Value, explanations: &mut Vec<String>) -> Vec<PathBuf> {
    let Some(items) = raw.as_array() else {
        explanations.push("Invalid dynare.searchPaths; use no search paths.".to_owned());
        return Vec::new();
    };
    let mut paths = Vec::new();
    for item in items {
        if let Some(text) = item.as_str().map(str::trim).filter(|s| !s.is_empty()) {
            let path = PathBuf::from(text);
            if !paths.contains(&path) {
                paths.push(path);
            }
        } else {
            explanations.push("Invalid dynare.searchPaths entry; ignore this entry.".to_owned());
        }
    }
    paths
}
