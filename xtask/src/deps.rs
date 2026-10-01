use std::collections::{HashMap, VecDeque};
use std::fmt;

use anyhow::Context as _;
use cargo_metadata::Metadata;

pub const UI_CRATE: &str = "bardo-ui";

/// The GPUI family on crates.io: `gpui`, `gpui-pre*`, `gpui-kit*`,
/// `gpui-base`, `gpui-component*`, and anything else named `gpui-…`.
pub fn is_gpui(package: &str) -> bool {
    package == "gpui" || package.starts_with("gpui-") || package.starts_with("gpui_")
}

/// Resolved dependency graph by package name, as far as this lint needs it.
#[derive(Debug, Default)]
pub struct Graph {
    members: Vec<String>,
    edges: HashMap<String, Vec<String>>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Violation {
    /// Dependency chain from the offending member to the GPUI crate.
    pub path: Vec<String>,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.path.join(" -> "))
    }
}

impl Graph {
    pub fn from_metadata(metadata: &Metadata) -> anyhow::Result<Self> {
        let resolve = metadata
            .resolve
            .as_ref()
            .context("cargo metadata returned no dependency resolution")?;
        let name_of = |id| metadata[id].name.to_string();
        let mut graph = Graph::default();
        for node in &resolve.nodes {
            graph.edges.insert(
                name_of(&node.id),
                node.deps.iter().map(|dep| name_of(&dep.pkg)).collect(),
            );
        }
        graph.members = metadata.workspace_members.iter().map(name_of).collect();
        Ok(graph)
    }

    /// One violation per workspace member (other than `allowed`) that reaches
    /// a GPUI crate, with the shortest chain that proves it.
    pub fn gpui_violations(&self, allowed: &str) -> Vec<Violation> {
        let mut members: Vec<_> = self.members.iter().filter(|m| *m != allowed).collect();
        members.sort();
        members
            .into_iter()
            .filter_map(|member| self.shortest_path_to_gpui(member))
            .map(|path| Violation { path })
            .collect()
    }

    fn shortest_path_to_gpui(&self, from: &str) -> Option<Vec<String>> {
        let mut parent: HashMap<&str, &str> = HashMap::new();
        let mut queue = VecDeque::from([from]);
        while let Some(current) = queue.pop_front() {
            for dep in self.edges.get(current).into_iter().flatten() {
                if dep == from || parent.contains_key(dep.as_str()) {
                    continue;
                }
                parent.insert(dep, current);
                if is_gpui(dep) {
                    let mut path = vec![dep.as_str()];
                    while let Some(&previous) = parent.get(path[path.len() - 1]) {
                        path.push(previous);
                    }
                    path.reverse();
                    return Some(path.into_iter().map(str::to_owned).collect());
                }
                queue.push_back(dep);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(members: &[&str], edges: &[(&str, &[&str])]) -> Graph {
        Graph {
            members: members.iter().map(|m| m.to_string()).collect(),
            edges: edges
                .iter()
                .map(|(from, to)| (from.to_string(), to.iter().map(|t| t.to_string()).collect()))
                .collect(),
        }
    }

    fn paths(violations: Vec<Violation>) -> Vec<String> {
        violations.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn recognizes_the_gpui_family() {
        for name in [
            "gpui",
            "gpui-pre",
            "gpui-pre-platform",
            "gpui-kit",
            "gpui-base",
            "gpui-component",
            "gpui_platform",
        ] {
            assert!(is_gpui(name), "{name}");
        }
        for name in ["gpuix", "egui", "bardo-ui", "wgpu"] {
            assert!(!is_gpui(name), "{name}");
        }
    }

    #[test]
    fn ui_may_depend_on_gpui() {
        let g = graph(
            &["bardo-ui", "bardo-app"],
            &[
                ("bardo-ui", &["bardo-app", "gpui-kit"]),
                ("gpui-kit", &["gpui-pre"]),
            ],
        );
        assert!(g.gpui_violations(UI_CRATE).is_empty());
    }

    #[test]
    fn direct_dependency_outside_ui_is_a_violation() {
        let g = graph(
            &["bardo-ui", "bardo-domain"],
            &[("bardo-domain", &["gpui-pre"])],
        );
        assert_eq!(
            paths(g.gpui_violations(UI_CRATE)),
            ["bardo-domain -> gpui-pre"]
        );
    }

    #[test]
    fn transitive_dependency_reports_the_chain() {
        let g = graph(
            &["bardo-ui", "bardo-app", "bardo-media"],
            &[
                ("bardo-app", &["bardo-media", "serde"]),
                ("bardo-media", &["some-widgets"]),
                ("some-widgets", &["gpui-kit"]),
            ],
        );
        assert_eq!(
            paths(g.gpui_violations(UI_CRATE)),
            [
                "bardo-app -> bardo-media -> some-widgets -> gpui-kit",
                "bardo-media -> some-widgets -> gpui-kit",
            ]
        );
    }

    #[test]
    fn depending_on_the_ui_crate_counts_as_depending_on_gpui() {
        let g = graph(
            &["bardo-ui", "bardo-app"],
            &[("bardo-app", &["bardo-ui"]), ("bardo-ui", &["gpui-kit"])],
        );
        assert_eq!(
            paths(g.gpui_violations(UI_CRATE)),
            ["bardo-app -> bardo-ui -> gpui-kit"]
        );
    }

    #[test]
    fn cycles_terminate() {
        let g = graph(&["a", "bardo-ui"], &[("a", &["b"]), ("b", &["a"])]);
        assert!(g.gpui_violations(UI_CRATE).is_empty());
    }

    /// The real workspace passes: this is the same check CI runs.
    #[test]
    fn this_workspace_passes() {
        let metadata = cargo_metadata::MetadataCommand::new()
            .manifest_path(concat!(env!("CARGO_MANIFEST_DIR"), "/../Cargo.toml"))
            .features(cargo_metadata::CargoOpt::AllFeatures)
            .exec()
            .unwrap();
        let violations = Graph::from_metadata(&metadata)
            .unwrap()
            .gpui_violations(UI_CRATE);
        assert!(violations.is_empty(), "{violations:?}");
    }
}
