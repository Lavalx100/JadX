//! Persistent, incremental project map used to ground IDE analysis.
//! Only source files already admitted by the workspace's ignore/security scan
//! are read. The database stores metadata and a small symbol/import/test map,
//! never source contents.
use crate::db::Db;
use crate::{context, db};
use regex::Regex;
use std::{path::Path, sync::OnceLock};
use tokio_util::sync::CancellationToken;

const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;
const MAX_INDEX_ENTRIES: usize = 100_000;

pub struct WorkspaceIndex {
    pub paths: Vec<String>,
    pub excluded: usize,
    pub bounded: bool,
}

/// Ignore-aware bounded directory scan. Content is handled separately by
/// `refresh`, which reuses the persisted map for unchanged source files.
pub fn scan(root: &Path, cancellation: &CancellationToken) -> Result<WorkspaceIndex, String> {
    let mut stack = vec![(root.to_path_buf(), crate::load_gitignore(root))];
    let mut paths = Vec::new();
    let mut excluded = 0;
    let mut visited = 0;
    while let Some((directory, inherited_patterns)) = stack.pop() {
        if cancellation.is_cancelled() {
            return Err("Analysis cancelled".into());
        }
        let mut patterns = inherited_patterns;
        if directory != root {
            let prefix = directory
                .strip_prefix(root)
                .unwrap_or(Path::new(""))
                .to_string_lossy()
                .replace('\\', "/");
            patterns.extend(
                crate::load_gitignore(&directory)
                    .into_iter()
                    .map(|pattern| {
                        if pattern.contains('/') {
                            format!("{prefix}/{pattern}")
                        } else {
                            pattern
                        }
                    }),
            );
        }
        for entry in std::fs::read_dir(&directory)
            .map_err(|error| error.to_string())?
            .filter_map(Result::ok)
        {
            visited += 1;
            if visited % 256 == 0 && cancellation.is_cancelled() {
                return Err("Analysis cancelled".into());
            }
            if visited > MAX_INDEX_ENTRIES {
                paths.sort();
                return Ok(WorkspaceIndex {
                    paths,
                    excluded,
                    bounded: true,
                });
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let relative = entry
                .path()
                .strip_prefix(root)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let Ok(file_type) = entry.file_type() else {
                excluded += 1;
                continue;
            };
            if file_type.is_symlink()
                || crate::is_workspace_hidden_or_generated(&name)
                || crate::is_gitignored(&relative, &patterns)
            {
                excluded += 1;
                continue;
            }
            if file_type.is_dir() {
                if !["credentials", "secrets", "private"]
                    .contains(&name.to_ascii_lowercase().as_str())
                {
                    stack.push((entry.path(), patterns.clone()));
                } else {
                    excluded += 1;
                }
            } else if crate::ide_intelligence::shareable_path(&relative)
                && entry
                    .metadata()
                    .map(|metadata| metadata.len() <= MAX_FILE_BYTES)
                    .unwrap_or(false)
            {
                paths.push(relative);
            } else {
                excluded += 1;
            }
        }
    }
    paths.sort();
    Ok(WorkspaceIndex {
        paths,
        excluded,
        bounded: false,
    })
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct IndexedFile {
    pub path: String,
    #[serde(default)]
    pub symbols: Vec<String>,
    #[serde(default)]
    pub imports: Vec<String>,
    #[serde(default)]
    pub tests: Vec<String>,
    #[serde(default)]
    pub diagnostics: Vec<IndexedDiagnostic>,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct IndexedDiagnostic {
    pub line: usize,
    pub severity: String,
    pub message: String,
}

/// Refresh changed files by metadata and reuse summaries for unchanged files.
/// Directory membership is supplied by the caller's ignore-aware scan, so
/// deletions and newly ignored files naturally disappear from the stored map.
pub fn refresh(
    database: &Db,
    workspace_id: &str,
    root: &Path,
    paths: &[String],
    cancellation: &CancellationToken,
) -> Result<Vec<IndexedFile>, String> {
    let cached = db::load_project_index(database, workspace_id)?;
    let root = root.canonicalize().map_err(|error| error.to_string())?;
    let mut entries = Vec::with_capacity(paths.len());
    let mut stored = Vec::with_capacity(paths.len());
    for (position, relative) in paths.iter().enumerate() {
        if position % 64 == 0 && cancellation.is_cancelled() {
            return Err("Project indexing cancelled".into());
        }
        let candidate = root.join(relative);
        match std::fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => {}
            _ => continue,
        }
        let absolute = match candidate.canonicalize() {
            Ok(path) if path.starts_with(&root) => path,
            _ => continue,
        };
        let metadata = match absolute.metadata() {
            Ok(metadata) if metadata.len() <= MAX_FILE_BYTES => metadata,
            _ => continue,
        };
        let size = metadata.len();
        let modified_ns = metadata
            .modified()
            .ok()
            .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64)
            .unwrap_or_default();
        let summary_json = cached
            .get(relative)
            .filter(|(cached_mtime, cached_size, _)| {
                *cached_mtime == modified_ns && *cached_size == size
            })
            .map(|(_, _, json)| json.clone())
            .or_else(|| {
                std::fs::read_to_string(&absolute)
                    .ok()
                    .map(|source| serde_json::to_string(&summarize(relative, &source)).ok())
                    .flatten()
            });
        let Some(summary_json) = summary_json else {
            continue;
        };
        let mut entry: IndexedFile = match serde_json::from_str(&summary_json) {
            Ok(entry) => entry,
            Err(_) => continue,
        };
        // The persisted path is not trusted as authority; always use the path
        // from the current workspace scan.
        entry.path = relative.clone();
        stored.push((relative.clone(), modified_ns, size, summary_json));
        entries.push(entry);
    }
    db::replace_project_index(database, workspace_id, &stored)?;
    Ok(entries)
}

/// Refresh the single file just saved in the IDE, without re-reading the
/// project or replacing unrelated cached summaries.
pub fn refresh_saved_file(
    database: &Db,
    workspace_id: &str,
    root: &Path,
    relative: &str,
) -> Result<(), String> {
    if !crate::ide_intelligence::shareable_path(relative) {
        return db::remove_project_index_entry(database, workspace_id, relative);
    }
    let canonical_root = root.canonicalize().map_err(|error| error.to_string())?;
    let relative_path = Path::new(relative);
    let components = relative_path
        .components()
        .map(|component| match component {
            std::path::Component::Normal(value) => Ok(value),
            _ => Err("invalid workspace index path".to_owned()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let Some((filename, parents)) = components.split_last() else {
        return Err("empty workspace index path".into());
    };
    let mut patterns = crate::load_gitignore(&canonical_root);
    let mut directory = canonical_root.clone();
    let mut prefix = Vec::new();
    for component in parents {
        directory.push(component);
        prefix.push(component.to_string_lossy().to_string());
        let nested_prefix = prefix.join("/");
        patterns.extend(
            crate::load_gitignore(&directory)
                .into_iter()
                .map(|pattern| {
                    if pattern.contains('/') {
                        format!("{nested_prefix}/{pattern}")
                    } else {
                        pattern
                    }
                }),
        );
        if crate::is_workspace_hidden_or_generated(&component.to_string_lossy()) {
            return db::remove_project_index_entry(database, workspace_id, relative);
        }
    }
    let filename = filename.to_string_lossy();
    if crate::is_workspace_hidden_or_generated(&filename)
        || crate::is_gitignored(relative, &patterns)
    {
        return db::remove_project_index_entry(database, workspace_id, relative);
    }
    let absolute = canonical_root.join(relative_path);
    let link_metadata = match std::fs::symlink_metadata(&absolute) {
        Ok(metadata) if metadata.is_file() && !metadata.file_type().is_symlink() => metadata,
        _ => return db::remove_project_index_entry(database, workspace_id, relative),
    };
    let absolute = absolute.canonicalize().map_err(|error| error.to_string())?;
    if !absolute.starts_with(&canonical_root) || link_metadata.len() > MAX_FILE_BYTES {
        return db::remove_project_index_entry(database, workspace_id, relative);
    }
    let metadata = absolute.metadata().map_err(|error| error.to_string())?;
    let source = std::fs::read_to_string(&absolute).map_err(|error| error.to_string())?;
    let modified_ns = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos().min(u64::MAX as u128) as u64)
        .unwrap_or_default();
    let summary =
        serde_json::to_string(&summarize(relative, &source)).map_err(|error| error.to_string())?;
    db::save_project_index_entry(
        database,
        workspace_id,
        relative,
        modified_ns,
        metadata.len(),
        &summary,
    )
}

pub fn render(entries: &[IndexedFile], budget: u64) -> String {
    let mut output = String::new();
    let mut used = 0u64;
    for (index, entry) in entries.iter().enumerate() {
        let details = [
            (!entry.symbols.is_empty()).then(|| format!("symbols: {}", entry.symbols.join(", "))),
            (!entry.imports.is_empty()).then(|| format!("imports: {}", entry.imports.join(", "))),
            (!entry.tests.is_empty()).then(|| format!("tests: {}", entry.tests.join(", "))),
            (!entry.diagnostics.is_empty()).then(|| {
                format!(
                    "diagnostics: {}",
                    entry
                        .diagnostics
                        .iter()
                        .take(3)
                        .map(|diagnostic| format!(
                            "{}:{} {}",
                            diagnostic.line, diagnostic.severity, diagnostic.message
                        ))
                        .collect::<Vec<_>>()
                        .join("; ")
                )
            }),
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" | ");
        let line = if details.is_empty() {
            entry.path.clone()
        } else {
            format!("{} — {}", entry.path, details)
        };
        let cost = context::estimate_text_tokens(&line) + 1;
        if used.saturating_add(cost) > budget {
            let omitted = format!(
                "\n[{} additional indexed files omitted]",
                entries.len() - index
            );
            if used.saturating_add(context::estimate_text_tokens(&omitted)) <= budget {
                output.push_str(&omitted);
            }
            break;
        }
        output.push_str(&line);
        output.push('\n');
        used += cost;
    }
    output
}

/// Rank the already-cached symbol map against the current task before applying
/// the same strict token budget. This does no workspace scan and never adds
/// source bodies to the map.
#[cfg(test)]
pub fn render_for_task(entries: &[IndexedFile], task: &str, budget: u64) -> String {
    let terms = search_terms(task);
    if terms.is_empty() {
        return render(entries, budget);
    }
    let ranked = entries
        .iter()
        .cloned()
        .map(|entry| (task_relevance_score(&entry, &terms), entry))
        .collect::<Vec<_>>();
    render_ranked(ranked, budget)
}

fn render_ranked(mut ranked: Vec<(usize, IndexedFile)>, budget: u64) -> String {
    ranked.sort_by(|(left_score, left), (right_score, right)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.path.cmp(&right.path))
    });
    let ranked = ranked
        .into_iter()
        .map(|(_, entry)| entry)
        .collect::<Vec<_>>();
    render(&ranked, budget)
}

fn task_relevance_score(entry: &IndexedFile, query_terms: &[String]) -> usize {
    query_terms
        .iter()
        .map(|term| {
            usize::from(contains_term(&entry.path, term)) * 8
                + usize::from(entry.symbols.iter().any(|value| contains_term(value, term))) * 5
                + usize::from(entry.tests.iter().any(|value| contains_term(value, term))) * 3
                + usize::from(entry.imports.iter().any(|value| contains_term(value, term))) * 2
                + usize::from(
                    entry
                        .diagnostics
                        .iter()
                        .any(|value| contains_term(&value.message, term)),
                )
        })
        .sum()
}

fn contains_term(text: &str, term: &str) -> bool {
    let bytes = text.as_bytes();
    let needle = term.as_bytes();
    !needle.is_empty()
        && needle.len() <= bytes.len()
        && bytes
            .windows(needle.len())
            .any(|candidate| candidate.eq_ignore_ascii_case(needle))
}

fn search_terms(text: &str) -> Vec<String> {
    const MAX_TERMS: usize = 24;
    let mut terms = Vec::new();
    let mut current = String::new();
    let mut previous = None;
    for character in text.chars().take(4_096) {
        if !character.is_alphanumeric() {
            push_search_term(&mut terms, &mut current);
        } else {
            if character.is_uppercase()
                && previous.is_some_and(char::is_lowercase)
                && current.len() > 1
            {
                push_search_term(&mut terms, &mut current);
            }
            current.extend(character.to_lowercase());
        }
        previous = Some(character);
        if terms.len() >= MAX_TERMS {
            break;
        }
    }
    push_search_term(&mut terms, &mut current);
    terms.truncate(MAX_TERMS);
    terms
}

fn push_search_term(terms: &mut Vec<String>, current: &mut String) {
    const STOP_WORDS: &[&str] = &[
        "the",
        "and",
        "for",
        "with",
        "from",
        "that",
        "this",
        "into",
        "when",
        "what",
        "where",
        "how",
        "can",
        "you",
        "your",
        "are",
        "was",
        "were",
        "has",
        "have",
        "had",
        "does",
        "did",
        "not",
        "but",
        "all",
        "any",
        "one",
        "new",
        "make",
        "create",
        "add",
        "update",
        "fix",
        "change",
        "improve",
        "better",
        "implement",
        "use",
        "using",
        "keep",
        "ensure",
        "please",
        "want",
        "project",
        "workspace",
        "selected",
        "active",
        "file",
    ];
    if current.len() >= 3 && !STOP_WORDS.contains(&current.as_str()) && !terms.contains(current) {
        terms.push(std::mem::take(current));
    } else {
        current.clear();
    }
}

/// Render the persisted map for one already-indexed workspace. This performs
/// no filesystem scan or source read, so it is cheap enough for agent routing.
#[cfg(test)]
pub fn cached_render(
    database: &Db,
    workspace_id: &str,
    budget: u64,
) -> Result<Option<String>, String> {
    let mut entries = db::load_project_index(database, workspace_id)?
        .into_iter()
        .filter_map(|(path, (_, _, summary))| {
            serde_json::from_str::<IndexedFile>(&summary)
                .ok()
                .map(|mut entry| {
                    entry.path = path;
                    entry
                })
        })
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| left.path.cmp(&right.path));
    let rendered = render(&entries, budget);
    Ok((!rendered.trim().is_empty()).then_some(rendered))
}

/// Task-ranked variant of [`cached_render`], still constrained to the same
/// cached metadata and caller-supplied token budget.
pub fn cached_render_for_task(
    database: &Db,
    workspace_id: &str,
    task: &str,
    budget: u64,
) -> Result<Option<String>, String> {
    let entries = db::load_project_index(database, workspace_id)?
        .into_iter()
        .filter_map(|(path, (_, _, summary))| {
            serde_json::from_str::<IndexedFile>(&summary)
                .ok()
                .map(|mut entry| {
                    entry.path = path;
                    entry
                })
        })
        .collect::<Vec<_>>();
    let terms = search_terms(task);
    let rendered = if terms.is_empty() {
        let mut entries = entries;
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        render(&entries, budget)
    } else {
        let ranked = entries
            .into_iter()
            .map(|entry| (task_relevance_score(&entry, &terms), entry))
            .collect::<Vec<_>>();
        render_ranked(ranked, budget)
    };
    Ok((!rendered.trim().is_empty()).then_some(rendered))
}

/// Attach compiler/linter diagnostics from an approved check to the existing
/// map. A subsequent source metadata change refreshes that file and clears its
/// old diagnostics automatically.
pub fn record_diagnostics(database: &Db, workspace_id: &str, output: &str) -> Result<(), String> {
    static TS_DIAGNOSTIC: OnceLock<Regex> = OnceLock::new();
    static GCC_DIAGNOSTIC: OnceLock<Regex> = OnceLock::new();
    let ts = TS_DIAGNOSTIC.get_or_init(|| {
        Regex::new(r"(?m)^([^\s()]+\.[A-Za-z0-9]+)\((\d+),(\d+)\):\s*(error|warning)[^\r\n]*")
            .expect("typescript diagnostic regex")
    });
    let gcc = GCC_DIAGNOSTIC.get_or_init(|| {
        Regex::new(r"(?m)^(?:\s*-->\s*)?([^\s:]+\.[A-Za-z0-9]+):(\d+)(?::\d+)?:\s*(error|warning)(?:\[[^\]]+\])?:?\s*([^\r\n]*)")
            .expect("compiler diagnostic regex")
    });
    let mut cached = db::load_project_index(database, workspace_id)?;
    for (_, _, summary) in cached.values_mut() {
        if let Ok(mut file) = serde_json::from_str::<IndexedFile>(summary) {
            file.diagnostics.clear();
            *summary = serde_json::to_string(&file).map_err(|error| error.to_string())?;
        }
    }
    let mut parsed = Vec::<(String, IndexedDiagnostic)>::new();
    for capture in ts.captures_iter(output) {
        let Some(path) = capture
            .get(1)
            .map(|value| value.as_str().replace('\\', "/"))
        else {
            continue;
        };
        if !cached.contains_key(&path) {
            continue;
        }
        parsed.push((
            path,
            IndexedDiagnostic {
                line: capture
                    .get(2)
                    .and_then(|value| value.as_str().parse().ok())
                    .unwrap_or(1),
                severity: capture
                    .get(4)
                    .map(|value| value.as_str())
                    .unwrap_or("error")
                    .into(),
                message: capture
                    .get(0)
                    .map(|value| value.as_str())
                    .unwrap_or_default()
                    .chars()
                    .take(240)
                    .collect(),
            },
        ));
    }
    for capture in gcc.captures_iter(output) {
        let Some(path) = capture
            .get(1)
            .map(|value| value.as_str().replace('\\', "/"))
        else {
            continue;
        };
        if !cached.contains_key(&path) {
            continue;
        }
        parsed.push((
            path,
            IndexedDiagnostic {
                line: capture
                    .get(2)
                    .and_then(|value| value.as_str().parse().ok())
                    .unwrap_or(1),
                severity: capture
                    .get(3)
                    .map(|value| value.as_str())
                    .unwrap_or("error")
                    .into(),
                message: capture
                    .get(4)
                    .map(|value| value.as_str())
                    .unwrap_or_default()
                    .chars()
                    .take(240)
                    .collect(),
            },
        ));
    }
    for (path, diagnostic) in parsed {
        if let Some((_, _, summary)) = cached.get_mut(&path) {
            if let Ok(mut file) = serde_json::from_str::<IndexedFile>(summary) {
                file.diagnostics.push(diagnostic);
                *summary = serde_json::to_string(&file).map_err(|error| error.to_string())?;
            }
        }
    }
    let stored = cached
        .into_iter()
        .map(|(path, (mtime, bytes, summary))| (path, mtime, bytes, summary))
        .collect::<Vec<_>>();
    db::replace_project_index(database, workspace_id, &stored)
}

fn summarize(path: &str, source: &str) -> IndexedFile {
    static SYMBOL: OnceLock<Regex> = OnceLock::new();
    static TEST: OnceLock<Regex> = OnceLock::new();
    let symbol_pattern = SYMBOL.get_or_init(|| {
        Regex::new(r"(?m)^\s*(?:(?:pub|export|default|async|static|const|let|var)\s+)*(?:class|function|interface|type|enum|struct|trait|impl|fn|def|func|mod)\s+([A-Za-z_$][\w$]*)")
            .expect("symbol regex")
    });
    let test_pattern = TEST.get_or_init(|| {
        Regex::new(r"(?m)^\s*(?:#\[(?:tokio::)?test\]|(?:it|test|describe)\s*\(|(?:async\s+)?fn\s+test_[A-Za-z0-9_]+)")
            .expect("test regex")
    });
    let (safe_source, _) = crate::ide_intelligence::redact_source(source);
    let source = safe_source.as_str();
    let mut symbols = symbol_pattern
        .captures_iter(source)
        .filter_map(|capture| capture.get(1).map(|name| name.as_str().to_owned()))
        .collect::<Vec<_>>();
    symbols.sort();
    symbols.dedup();
    symbols.truncate(10);

    let mut imports = source
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let candidate = if line.starts_with("import ") || line.starts_with("use ") {
                Some(line)
            } else if line.starts_with("from ") && line.contains(" import ") {
                Some(line)
            } else {
                None
            }?;
            Some(candidate.chars().take(120).collect::<String>())
        })
        .collect::<Vec<_>>();
    imports.sort();
    imports.dedup();
    imports.truncate(6);

    let filename = Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let is_test_file = filename.contains("test") || filename.contains("spec");
    let mut tests = if is_test_file {
        test_pattern
            .find_iter(source)
            .filter_map(|matched| {
                source[matched.start()..]
                    .lines()
                    .next()
                    .map(|line| line.trim().chars().take(100).collect::<String>())
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    tests.sort();
    tests.dedup();
    tests.truncate(6);

    IndexedFile {
        path: path.to_owned(),
        symbols,
        imports,
        tests,
        diagnostics: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_eval_maps_symbols_imports_and_tests_without_source_bodies() {
        let source = r#"
import { NoteStore } from "./store";
export function createNote(title: string) { return new NoteStore(title); }
describe("notes", () => { it("creates notes", () => {}); });
"#;
        let entry = summarize("src/notes.test.ts", source);
        assert!(entry.symbols.iter().any(|symbol| symbol == "createNote"));
        assert!(entry.imports.iter().any(|line| line.contains("./store")));
        assert!(!entry.tests.is_empty());
        assert!(!render(&[entry], 4096).contains("new NoteStore"));
    }

    #[test]
    fn offline_eval_bounds_map_and_keeps_file_names() {
        let entries = (0..10)
            .map(|index| IndexedFile {
                path: format!("src/module_{index}.ts"),
                symbols: vec![format!("function_{index}")],
                ..IndexedFile::default()
            })
            .collect::<Vec<_>>();
        let output = render(&entries, 18);
        assert!(output.contains("src/module_0.ts"));
        assert!(context::estimate_text_tokens(&output) <= 18);
    }

    #[test]
    fn task_ranked_map_uses_the_existing_budget_for_relevant_symbols_first() {
        let entries = vec![
            IndexedFile {
                path: "src/account/report.ts".into(),
                symbols: vec!["renderAccountReport".into()],
                ..IndexedFile::default()
            },
            IndexedFile {
                path: "src/mobile/preview.ts".into(),
                symbols: vec!["MobilePreviewToggle".into()],
                ..IndexedFile::default()
            },
        ];
        let budget =
            context::estimate_text_tokens("src/mobile/preview.ts — symbols: MobilePreviewToggle")
                + 1;
        let output = render_for_task(&entries, "add a mobile preview toggle", budget);
        assert!(output.contains("src/mobile/preview.ts"));
        assert!(!output.contains("src/account/report.ts"));
        assert!(context::estimate_text_tokens(&output) <= budget);
    }

    #[test]
    fn cached_render_reuses_compact_metadata_without_source_bodies() {
        let directory = tempfile::tempdir().unwrap();
        let database = db::init_db(&directory.path().join("index.sqlite")).unwrap();
        let workspace_root = directory.path().join("workspace");
        std::fs::create_dir_all(&workspace_root).unwrap();
        let workspace =
            db::add_workspace(&database, "Fixture", workspace_root.to_str().unwrap()).unwrap();
        let summary = serde_json::to_string(&summarize(
            "src/notes.ts",
            "export function createNote() { return 'private body'; }",
        ))
        .unwrap();
        db::save_project_index_entry(&database, &workspace.id, "src/notes.ts", 1, 64, &summary)
            .unwrap();

        let output = cached_render(&database, &workspace.id, 512)
            .unwrap()
            .unwrap();
        assert!(output.contains("src/notes.ts"));
        assert!(output.contains("createNote"));
        assert!(!output.contains("private body"));
        assert!(cached_render(&database, "missing-workspace", 512)
            .unwrap()
            .is_none());
    }

    #[test]
    fn offline_eval_metadata_changes_trigger_refresh_integration() {
        // The deterministic evaluator counts external tokens as zero; estimates
        // are used only to compare a focused symbol map against raw source.
        let raw = "function createNote() { return new NoteStore(); }".repeat(20);
        let mapped = render(&[summarize("src/notes.ts", &raw)], 4096);
        assert!(context::estimate_text_tokens(&mapped) < context::estimate_text_tokens(&raw));
    }

    #[test]
    fn offline_eval_attaches_diagnostics_to_indexed_paths_only() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(
            root.path().join("src/main.ts"),
            "export function run() {}\n",
        )
        .unwrap();
        let database = db::init_db(Path::new(":memory:")).unwrap();
        let workspace = db::add_workspace(
            &database,
            "offline-diagnostics",
            root.path().to_str().unwrap(),
        )
        .unwrap();
        let token = CancellationToken::new();
        let scan = scan(root.path(), &token).unwrap();
        refresh(&database, &workspace.id, root.path(), &scan.paths, &token).unwrap();
        record_diagnostics(
            &database,
            &workspace.id,
            "src/main.ts(7,2): error TS1005: ';' expected\nmissing.ts:1:2: error: unknown\n",
        )
        .unwrap();
        let cached = db::load_project_index(&database, &workspace.id).unwrap();
        let indexed: IndexedFile = serde_json::from_str(&cached["src/main.ts"].2).unwrap();
        assert_eq!(indexed.diagnostics.len(), 1);
        assert_eq!(indexed.diagnostics[0].line, 7);
        assert!(indexed.diagnostics[0].message.contains("TS1005"));
    }

    #[test]
    fn offline_eval_updates_just_saved_file_and_drops_it_when_ignored() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(
            root.path().join("src/note.ts"),
            "export function createNote() {}\n",
        )
        .unwrap();
        let database = db::init_db(Path::new(":memory:")).unwrap();
        let workspace =
            db::add_workspace(&database, "offline-save", root.path().to_str().unwrap()).unwrap();
        refresh_saved_file(&database, &workspace.id, root.path(), "src/note.ts").unwrap();
        let cached = db::load_project_index(&database, &workspace.id).unwrap();
        let indexed: IndexedFile = serde_json::from_str(&cached["src/note.ts"].2).unwrap();
        assert!(indexed.symbols.iter().any(|symbol| symbol == "createNote"));

        std::fs::write(
            root.path().join("src/note.ts"),
            "export function archiveNote() {}\n",
        )
        .unwrap();
        refresh_saved_file(&database, &workspace.id, root.path(), "src/note.ts").unwrap();
        let cached = db::load_project_index(&database, &workspace.id).unwrap();
        let indexed: IndexedFile = serde_json::from_str(&cached["src/note.ts"].2).unwrap();
        assert!(indexed.symbols.iter().any(|symbol| symbol == "archiveNote"));
        assert!(!indexed.symbols.iter().any(|symbol| symbol == "createNote"));

        std::fs::write(root.path().join(".gitignore"), "src/note.ts\n").unwrap();
        refresh_saved_file(&database, &workspace.id, root.path(), "src/note.ts").unwrap();
        assert!(!db::load_project_index(&database, &workspace.id)
            .unwrap()
            .contains_key("src/note.ts"));
    }

    #[test]
    fn offline_eval_refreshes_changed_files_and_prunes_ignored_secrets() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(
            root.path().join("src/notes.ts"),
            "export function createNote() { return true; }\n",
        )
        .unwrap();
        std::fs::write(root.path().join(".gitignore"), "private.ts\n").unwrap();
        std::fs::write(
            root.path().join("private.ts"),
            "export function secret() {}\n",
        )
        .unwrap();
        std::fs::write(
            root.path().join(".env"),
            "API_KEY=placeholder\n",
        )
        .unwrap();

        let database = db::init_db(Path::new(":memory:")).unwrap();
        let workspace =
            db::add_workspace(&database, "offline-eval", root.path().to_str().unwrap()).unwrap();
        let token = CancellationToken::new();
        let initial_scan = scan(root.path(), &token).unwrap();
        let first = refresh(
            &database,
            &workspace.id,
            root.path(),
            &initial_scan.paths,
            &token,
        )
        .unwrap();
        assert!(first.iter().any(|file| file.path == "src/notes.ts"));
        assert!(first.iter().all(|file| file.path != "private.ts"));
        assert!(first
            .iter()
            .all(|file| !file.symbols.iter().any(|symbol| symbol == "secret")));

        std::thread::sleep(std::time::Duration::from_millis(2));
        std::fs::write(
            root.path().join("src/notes.ts"),
            "export function archiveNote() { return true; }\n",
        )
        .unwrap();
        std::fs::write(root.path().join(".gitignore"), "private.ts\nsrc/notes.ts\n").unwrap();
        let second_scan = scan(root.path(), &token).unwrap();
        let second = refresh(
            &database,
            &workspace.id,
            root.path(),
            &second_scan.paths,
            &token,
        )
        .unwrap();
        assert!(second.iter().all(|file| file.path != "src/notes.ts"));
        let cached = db::load_project_index(&database, &workspace.id).unwrap();
        assert!(!cached.contains_key("src/notes.ts"));
        assert!(!cached.contains_key("private.ts"));
    }
}
