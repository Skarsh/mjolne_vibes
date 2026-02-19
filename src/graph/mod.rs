use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};

pub mod watch;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchitectureGraph {
    pub nodes: Vec<ArchitectureNode>,
    pub edges: Vec<ArchitectureEdge>,
    pub revision: u64,
    pub generated_at: SystemTime,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ArchitectureNode {
    pub id: String,
    pub display_label: String,
    pub kind: ArchitectureNodeKind,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ArchitectureNodeKind {
    File,
    Module,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ArchitectureEdge {
    pub from: String,
    pub to: String,
    pub relation: ArchitectureEdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ArchitectureEdgeKind {
    DefinesModule,
    DeclaresModule,
    ResolvesToFile,
}

pub fn build_rust_workspace_graph(
    workspace_root: &Path,
    revision: u64,
) -> Result<ArchitectureGraph> {
    build_rust_workspace_graph_at(workspace_root, revision, SystemTime::now())
}

pub fn build_rust_workspace_graph_at(
    workspace_root: &Path,
    revision: u64,
    generated_at: SystemTime,
) -> Result<ArchitectureGraph> {
    ensure!(
        workspace_root.is_dir(),
        "workspace root must be a directory: {}",
        workspace_root.display()
    );

    let rust_files = collect_rust_files(workspace_root)?;
    let rust_file_set = rust_files.iter().cloned().collect::<BTreeSet<_>>();

    let mut nodes = BTreeMap::<String, ArchitectureNode>::new();
    let mut edges = BTreeSet::<ArchitectureEdge>::new();

    for relative_path in &rust_files {
        let file_id = file_node_id(relative_path);
        nodes.insert(
            file_id.clone(),
            ArchitectureNode {
                id: file_id.clone(),
                display_label: relative_path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default()
                    .to_owned(),
                kind: ArchitectureNodeKind::File,
                path: Some(path_to_slash_string(relative_path)),
            },
        );

        let module_path = module_path_for_file(relative_path);
        let module_id = module_node_id(&module_path);
        nodes.insert(
            module_id.clone(),
            ArchitectureNode {
                id: module_id.clone(),
                display_label: module_path.clone(),
                kind: ArchitectureNodeKind::Module,
                path: Some(path_to_slash_string(relative_path)),
            },
        );
        edges.insert(ArchitectureEdge {
            from: file_id,
            to: module_id.clone(),
            relation: ArchitectureEdgeKind::DefinesModule,
        });

        let source = fs::read_to_string(workspace_root.join(relative_path))
            .with_context(|| format!("failed to read `{}`", relative_path.display()))?;
        for declaration in parse_module_declarations(&source) {
            let child_path = format!("{module_path}::{}", declaration.name);
            let child_id = module_node_id(&child_path);

            nodes
                .entry(child_id.clone())
                .or_insert_with(|| ArchitectureNode {
                    id: child_id.clone(),
                    display_label: child_path,
                    kind: ArchitectureNodeKind::Module,
                    path: None,
                });
            edges.insert(ArchitectureEdge {
                from: module_id.clone(),
                to: child_id.clone(),
                relation: ArchitectureEdgeKind::DeclaresModule,
            });

            if declaration.inline {
                continue;
            }

            if let Some(resolved_relative_file) =
                resolve_declared_module_file(relative_path, &declaration.name, &rust_file_set)
            {
                let resolved_file_id = file_node_id(&resolved_relative_file);
                edges.insert(ArchitectureEdge {
                    from: child_id,
                    to: resolved_file_id,
                    relation: ArchitectureEdgeKind::ResolvesToFile,
                });
            }
        }
    }

    Ok(ArchitectureGraph {
        nodes: nodes.into_values().collect(),
        edges: edges.into_iter().collect(),
        revision,
        generated_at,
    })
}

fn collect_rust_files(workspace_root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect_rust_files_recursive(workspace_root, workspace_root, &mut files)?;
    files.sort_by_key(|path| path_to_slash_string(path.as_path()));
    Ok(files)
}

fn collect_rust_files_recursive(
    workspace_root: &Path,
    current_dir: &Path,
    files: &mut Vec<PathBuf>,
) -> Result<()> {
    let mut entries = fs::read_dir(current_dir)
        .with_context(|| format!("failed to list directory `{}`", current_dir.display()))?
        .collect::<std::io::Result<Vec<_>>>()
        .with_context(|| format!("failed to read entries in `{}`", current_dir.display()))?;
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        let path = entry.path();
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to inspect `{}`", path.display()))?;
        if file_type.is_dir() {
            let name = entry.file_name();
            if should_skip_dir(name.to_string_lossy().as_ref()) {
                continue;
            }
            collect_rust_files_recursive(workspace_root, &path, files)?;
            continue;
        }

        if !file_type.is_file() || path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let relative_path = path.strip_prefix(workspace_root).with_context(|| {
            format!(
                "failed to strip workspace root `{}` from `{}`",
                workspace_root.display(),
                path.display()
            )
        })?;
        files.push(relative_path.to_path_buf());
    }

    Ok(())
}

fn should_skip_dir(name: &str) -> bool {
    matches!(
        name,
        "target" | ".git" | ".idea" | ".vscode" | "node_modules"
    )
}

fn file_node_id(relative_path: &Path) -> String {
    format!("file:{}", path_to_slash_string(relative_path))
}

fn module_node_id(module_path: &str) -> String {
    format!("module:{module_path}")
}

fn path_to_slash_string(path: &Path) -> String {
    let segments = path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    segments.join("/")
}

fn module_path_for_file(relative_path: &Path) -> String {
    let components = relative_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    if components.is_empty() {
        return "workspace".to_owned();
    }

    if components
        .first()
        .is_some_and(|component| component == "src")
    {
        return module_path_for_src_file(relative_path);
    }

    let mut module_parts = Vec::with_capacity(components.len());
    for component in components {
        if component.ends_with(".rs") {
            let stem = component.trim_end_matches(".rs");
            if stem != "mod" {
                module_parts.push(stem.to_owned());
            }
        } else {
            module_parts.push(component);
        }
    }

    if module_parts.is_empty() {
        "workspace".to_owned()
    } else {
        module_parts.join("::")
    }
}

fn module_path_for_src_file(relative_path: &Path) -> String {
    let rel = path_to_slash_string(relative_path);
    if rel == "src/lib.rs" {
        return "crate".to_owned();
    }
    if rel == "src/main.rs" {
        return "crate::main".to_owned();
    }

    let mut parts = vec!["crate".to_owned()];
    let components = relative_path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    for component in components.into_iter().skip(1) {
        if component.ends_with(".rs") {
            let stem = component.trim_end_matches(".rs");
            if stem != "mod" {
                parts.push(stem.to_owned());
            }
        } else {
            parts.push(component);
        }
    }

    parts.join("::")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ModuleDeclaration {
    name: String,
    inline: bool,
}

fn parse_module_declarations(source: &str) -> Vec<ModuleDeclaration> {
    let mut declarations = Vec::new();

    for line in source.lines() {
        let mut candidate = line.trim();
        if candidate.is_empty() || candidate.starts_with("//") {
            continue;
        }

        if let Some((before_comment, _)) = candidate.split_once("//") {
            candidate = before_comment.trim();
        }
        if candidate.is_empty() || !candidate.contains("mod ") {
            continue;
        }

        let Some(mod_start) = candidate.find("mod ") else {
            continue;
        };
        let prefix = candidate[..mod_start].trim();
        if !is_valid_mod_prefix(prefix) {
            continue;
        }

        let rest = &candidate[mod_start + 4..];
        let module_name = rest
            .chars()
            .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
            .collect::<String>();
        if module_name.is_empty() {
            continue;
        }

        let suffix = rest[module_name.len()..].trim_start();
        let inline = if suffix.starts_with('{') {
            true
        } else if suffix.starts_with(';') {
            false
        } else {
            continue;
        };

        declarations.push(ModuleDeclaration {
            name: module_name,
            inline,
        });
    }

    declarations
}

fn is_valid_mod_prefix(prefix: &str) -> bool {
    prefix.is_empty() || prefix.starts_with("pub")
}

fn resolve_declared_module_file(
    declaring_file: &Path,
    module_name: &str,
    known_files: &BTreeSet<PathBuf>,
) -> Option<PathBuf> {
    let parent_dir = declaring_file.parent()?;
    let declaring_stem = declaring_file.file_stem()?.to_str()?;
    let search_base =
        if declaring_stem == "mod" || declaring_stem == "lib" || declaring_stem == "main" {
            parent_dir.to_path_buf()
        } else {
            parent_dir.join(declaring_stem)
        };

    let file_candidate = search_base.join(format!("{module_name}.rs"));
    if known_files.contains(&file_candidate) {
        return Some(file_candidate);
    }

    let mod_candidate = search_base.join(module_name).join("mod.rs");
    if known_files.contains(&mod_candidate) {
        return Some(mod_candidate);
    }

    None
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Duration, UNIX_EPOCH};

    use crate::test_support::{remove_dir_if_exists, temp_path};

    use super::{
        ArchitectureEdgeKind, ArchitectureNodeKind, build_rust_workspace_graph_at,
        parse_module_declarations, resolve_declared_module_file,
    };

    #[test]
    fn parse_module_declarations_handles_inline_and_file_modules() {
        let declarations = parse_module_declarations(
            r#"
                mod alpha;
                pub mod beta;
                pub(crate) mod gamma;
                mod inline_mod {
                    pub fn value() {}
                }
                let_mod_name = "skip me";
            "#,
        );

        assert_eq!(declarations.len(), 4);
        assert_eq!(declarations[0].name, "alpha");
        assert!(!declarations[0].inline);
        assert_eq!(declarations[1].name, "beta");
        assert!(!declarations[1].inline);
        assert_eq!(declarations[2].name, "gamma");
        assert!(!declarations[2].inline);
        assert_eq!(declarations[3].name, "inline_mod");
        assert!(declarations[3].inline);
    }

    #[test]
    fn resolve_declared_module_file_supports_standard_layout_rules() {
        let known_files = BTreeSet::from([
            PathBuf::from("src/lib.rs"),
            PathBuf::from("src/alpha.rs"),
            PathBuf::from("src/beta/mod.rs"),
            PathBuf::from("src/nested/mod.rs"),
            PathBuf::from("src/nested/inner.rs"),
        ]);

        let alpha = resolve_declared_module_file(Path::new("src/lib.rs"), "alpha", &known_files)
            .expect("alpha should resolve");
        assert_eq!(alpha, PathBuf::from("src/alpha.rs"));

        let beta = resolve_declared_module_file(Path::new("src/lib.rs"), "beta", &known_files)
            .expect("beta should resolve");
        assert_eq!(beta, PathBuf::from("src/beta/mod.rs"));

        let nested_inner =
            resolve_declared_module_file(Path::new("src/nested/mod.rs"), "inner", &known_files)
                .expect("inner should resolve");
        assert_eq!(nested_inner, PathBuf::from("src/nested/inner.rs"));
    }

    #[test]
    fn build_rust_workspace_graph_is_deterministic_and_sorted() {
        let root = temp_path("graph-determinism");
        fs::create_dir_all(root.join("src")).expect("src directory should be created");
        fs::write(root.join("src/lib.rs"), "mod b;\nmod a;\n").expect("lib should be written");
        fs::write(root.join("src/a.rs"), "mod inner;\n").expect("a should be written");
        fs::create_dir_all(root.join("src/a")).expect("src/a directory should be created");
        fs::write(root.join("src/a/inner.rs"), "pub fn go() {}\n")
            .expect("inner should be written");
        fs::write(root.join("src/b.rs"), "pub fn b() {}\n").expect("b should be written");
        fs::create_dir_all(root.join("target")).expect("target directory should be created");
        fs::write(root.join("target/skip.rs"), "mod ghost;\n")
            .expect("skip file should be written");

        let timestamp = UNIX_EPOCH + Duration::from_secs(7);
        let graph_one = build_rust_workspace_graph_at(&root, 11, timestamp)
            .expect("graph build should succeed");
        let graph_two = build_rust_workspace_graph_at(&root, 11, timestamp)
            .expect("graph build should succeed");

        assert_eq!(graph_one, graph_two);
        assert_eq!(graph_one.revision, 11);
        assert_eq!(graph_one.generated_at, timestamp);

        let mut sorted_node_ids = graph_one
            .nodes
            .iter()
            .map(|node| node.id.clone())
            .collect::<Vec<_>>();
        let original_node_ids = sorted_node_ids.clone();
        sorted_node_ids.sort();
        assert_eq!(original_node_ids, sorted_node_ids);

        let mut sorted_edges = graph_one.edges.clone();
        let original_edges = sorted_edges.clone();
        sorted_edges.sort();
        assert_eq!(original_edges, sorted_edges);

        assert!(
            graph_one
                .nodes
                .iter()
                .all(|node| !node.id.contains("target/skip.rs")),
            "target directory files should be ignored"
        );

        remove_dir_if_exists(&root);
    }

    #[test]
    fn build_rust_workspace_graph_emits_expected_core_relations() {
        let root = temp_path("graph-relations");
        fs::create_dir_all(root.join("src/tools")).expect("tools directory should be created");
        fs::write(
            root.join("src/lib.rs"),
            "mod tools;\nmod inline { fn keep() {} }\n",
        )
        .expect("lib should be written");
        fs::write(root.join("src/tools/mod.rs"), "pub mod parser;\n")
            .expect("tools/mod.rs should be written");
        fs::write(root.join("src/tools/parser.rs"), "pub fn parse() {}\n")
            .expect("tools/parser.rs should be written");

        let graph = build_rust_workspace_graph_at(&root, 2, UNIX_EPOCH + Duration::from_secs(2))
            .expect("graph build should succeed");

        let file_nodes = graph
            .nodes
            .iter()
            .filter(|node| node.kind == ArchitectureNodeKind::File)
            .count();
        assert_eq!(file_nodes, 3);
        assert!(graph.nodes.iter().any(|node| {
            node.kind == ArchitectureNodeKind::Module && node.id == "module:crate"
        }));
        assert!(graph.nodes.iter().any(|node| {
            node.kind == ArchitectureNodeKind::Module && node.id == "module:crate::tools"
        }));
        assert!(graph.nodes.iter().any(|node| {
            node.kind == ArchitectureNodeKind::Module && node.id == "module:crate::inline"
        }));
        assert!(graph.nodes.iter().any(|node| {
            node.kind == ArchitectureNodeKind::Module && node.id == "module:crate::tools::parser"
        }));

        assert!(graph.edges.iter().any(|edge| {
            edge.from == "module:crate"
                && edge.to == "module:crate::tools"
                && edge.relation == ArchitectureEdgeKind::DeclaresModule
        }));
        assert!(graph.edges.iter().any(|edge| {
            edge.from == "module:crate::tools"
                && edge.to == "file:src/tools/mod.rs"
                && edge.relation == ArchitectureEdgeKind::ResolvesToFile
        }));
        assert!(graph.edges.iter().any(|edge| {
            edge.from == "module:crate::tools"
                && edge.to == "module:crate::tools::parser"
                && edge.relation == ArchitectureEdgeKind::DeclaresModule
        }));

        remove_dir_if_exists(&root);
    }

    #[test]
    fn graph_builder_rejects_missing_workspace_root() {
        let root = temp_path("graph-missing-root");
        let error =
            build_rust_workspace_graph_at(&root, 1, UNIX_EPOCH).expect_err("build should fail");
        assert!(
            error
                .to_string()
                .contains("workspace root must be a directory")
        );
    }
}
