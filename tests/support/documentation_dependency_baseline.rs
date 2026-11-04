// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::VecDeque;
use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

pub(crate) fn resolve_target_dir(package_root: &Path, configured: Option<&OsStr>) -> PathBuf {
    let target = configured
        .map(PathBuf::from)
        .unwrap_or_else(|| package_root.join("target"));
    if target.is_absolute() {
        target
    } else {
        package_root.join(target)
    }
}

pub(crate) fn read_lock_seed(path: &Path) -> std::io::Result<Vec<u8>> {
    std::fs::read(path)
}

pub(crate) fn root_fetch_command(cargo: &OsStr, root: &Path) -> Command {
    let mut command = Command::new(cargo);
    command
        .arg("fetch")
        .arg("--locked")
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .current_dir(root);
    command
}

pub(crate) fn root_metadata_command(cargo: &OsStr, root: &Path) -> Command {
    let mut command = Command::new(cargo);
    command
        .args(["metadata", "--format-version", "1", "--locked", "--offline"])
        .arg("--manifest-path")
        .arg(root.join("Cargo.toml"))
        .current_dir(root);
    command
}

pub(crate) fn harness_metadata_command(cargo: &OsStr, root: &Path, manifest: &Path) -> Command {
    let mut command = Command::new(cargo);
    command
        .args(["metadata", "--format-version", "1", "--offline"])
        .arg("--manifest-path")
        .arg(manifest)
        .current_dir(root);
    command
}

pub(crate) fn harness_test_command(cargo: &OsStr, root: &Path, manifest: &Path, target: &Path) -> Command {
    let mut command = Command::new(cargo);
    command
        .arg("test")
        .arg("--manifest-path")
        .arg(manifest)
        .arg("--target-dir")
        .arg(target)
        .arg("--locked")
        .arg("--offline")
        .current_dir(root);
    command
}

fn field<'a>(value: &'a Value, key: &str) -> Result<&'a Value, String> {
    value.get(key).ok_or_else(|| format!("metadata missing {key}"))
}

fn string<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    field(value, key)?
        .as_str()
        .ok_or_else(|| format!("metadata {key} is not a string"))
}

fn package_id(package: &Value, base: &Path) -> Result<String, String> {
    let name = string(package, "name")?;
    let version = string(package, "version")?;
    let source = package.get("source").and_then(Value::as_str);
    let origin = match source {
        Some(source) => source.to_owned(),
        None => {
            let manifest = Path::new(string(package, "manifest_path")?);
            let directory = manifest
                .parent()
                .ok_or_else(|| format!("invalid manifest path for {name}"))?;
            let path = if directory.is_absolute() {
                directory.to_path_buf()
            } else {
                base.join(directory)
            };
            format!("path+{}", path.canonicalize().unwrap_or(path).display())
        }
    };
    Ok(format!("{name}@{version} ({origin})"))
}

type DependencyEdge = (String, String, String, String);
type PackageSet = BTreeSet<String>;
type DependencyEdgeSet = BTreeSet<DependencyEdge>;
type Closure = (PackageSet, DependencyEdgeSet);
type ResolveNode = (String, Vec<DependencyEdge>);

#[derive(Default)]
struct Graph {
    nodes: BTreeMap<String, ResolveNode>,
    roots: Vec<String>,
}

fn production_graph(metadata: &Value, base: &Path) -> Result<Graph, String> {
    let packages = field(metadata, "packages")?
        .as_array()
        .ok_or("metadata packages is not an array")?;
    let resolve = field(metadata, "resolve")?;
    let resolve_nodes = field(resolve, "nodes")?
        .as_array()
        .ok_or("metadata resolve.nodes is not an array")?;
    let mut ids = BTreeMap::new();
    for package in packages {
        ids.insert(string(package, "id")?.to_owned(), package_id(package, base)?);
    }
    let roots = packages
        .iter()
        .filter(|package| string(package, "name").ok() == Some("qubit-command"))
        .map(|package| string(package, "id").map(str::to_owned))
        .collect::<Result<Vec<_>, _>>()?;
    if roots.len() != 1 {
        return Err(format!("expected one qubit-command package, found {}", roots.len()));
    }
    let mut graph = Graph {
        roots: roots.clone(),
        ..Graph::default()
    };
    for node in resolve_nodes {
        let id = string(node, "id")?.to_owned();
        let identity = ids
            .get(&id)
            .ok_or_else(|| format!("resolve node has unknown package {id}"))?
            .clone();
        let mut edges = Vec::new();
        for dependency in field(node, "deps")?.as_array().ok_or("node deps is not an array")? {
            let dep_name = string(dependency, "name")?.to_owned();
            let target_ids = field(dependency, "pkg")?
                .as_str()
                .ok_or("dependency pkg is not a string")?;
            let target = ids
                .get(target_ids)
                .ok_or_else(|| format!("dependency {dep_name} has unknown package {target_ids}"))?
                .clone();
            for kind in field(dependency, "dep_kinds")?
                .as_array()
                .ok_or("dep_kinds is not an array")?
            {
                let kind_value = field(kind, "kind")?;
                let kind_name = match kind_value {
                    Value::Null => "normal",
                    Value::String(value) if value == "build" => "build",
                    Value::String(value) if value == "dev" => continue,
                    Value::String(value) => {
                        return Err(format!("unsupported dependency kind {value}"));
                    }
                    _ => return Err("dependency kind is neither null nor a string".into()),
                };
                let target_condition = kind.get("target").and_then(Value::as_str).unwrap_or("");
                edges.push((
                    dep_name.clone(),
                    target.clone(),
                    kind_name.to_owned(),
                    target_condition.to_owned(),
                ));
            }
        }
        edges.sort();
        graph.nodes.insert(id, (identity, edges));
    }
    Ok(graph)
}

fn closure(graph: &Graph) -> Result<Closure, String> {
    let mut seen = BTreeSet::new();
    let mut edges = BTreeSet::new();
    let mut queue: VecDeque<_> = graph.roots.iter().cloned().collect();
    while let Some(id) = queue.pop_front() {
        if !seen.insert(id.clone()) {
            continue;
        }
        let (identity, outgoing) = graph
            .nodes
            .get(&id)
            .ok_or_else(|| format!("missing resolve node {id}"))?;
        for (name, target, kind, condition) in outgoing {
            edges.insert((
                identity.clone(),
                name.clone(),
                target.clone(),
                format!("{kind}:{condition}"),
            ));
            if let Some((target_id, _)) = graph.nodes.iter().find(|(_, (candidate, _))| candidate == target)
                && !seen.contains(target_id)
            {
                queue.push_back(target_id.clone());
            }
        }
    }
    let identities = seen
        .iter()
        .filter_map(|id| graph.nodes.get(id).map(|(identity, _)| identity.clone()))
        .collect();
    Ok((identities, edges))
}

pub(crate) fn compare_metadata(baseline: &Value, harness: &Value, base: &Path) -> Result<(), String> {
    let baseline_graph = production_graph(baseline, base)?;
    let harness_graph = production_graph(harness, base)?;
    let (base_packages, base_edges) = closure(&baseline_graph)?;
    let (harness_packages, harness_edges) = closure(&harness_graph)?;
    if let Some(package) = harness_packages.difference(&base_packages).next() {
        return Err(format!(
            "production dependency baseline drift: package {package} is absent"
        ));
    }
    if let Some((from, name, to, kind)) = harness_edges.difference(&base_edges).next() {
        return Err(format!(
            "production dependency baseline drift: {from} --{name} ({kind})--> {to} is absent"
        ));
    }
    Ok(())
}
