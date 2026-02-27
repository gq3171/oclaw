//! Task graph — DAG-based parallel task execution with conditional branching and data flow.
//!
//! ## Overview
//!
//! [`TaskGraph`] describes a directed acyclic graph of [`TaskNode`]s. Each node
//! is a subagent task with:
//!
//! * **`depends_on`** — prerequisite nodes that must *succeed* before this one can run.
//! * **`on_success` / `on_failure`** — conditional branching: which nodes become
//!   *eligible* to run depending on this node's outcome.
//! * **`input_from` + `input_template`** — data flow: pipe a predecessor's output
//!   into this node's input (with optional template substitution).
//!
//! [`TaskGraphRunner`] executes the graph concurrently (default max 3 in-flight),
//! respecting both the dependency order and the activation set maintained by
//! `on_success` / `on_failure` links.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use oclaw_llm_core::providers::LlmProvider;

use crate::agent::ToolExecutor;
use crate::subagent::{SubagentConfig, SubagentRegistry};

// ── Node ─────────────────────────────────────────────────────────────────────

/// A single node in the task graph.
pub struct TaskNode {
    /// Unique identifier for this node within the graph.
    pub id: String,

    /// Subagent configuration (model, system prompt, etc.).
    pub config: SubagentConfig,

    /// IDs of nodes that must successfully complete before this node can run.
    /// Empty means this node is a root (starts immediately when activated).
    pub depends_on: Vec<String>,

    /// Node IDs to activate when this node **succeeds**.
    pub on_success: Vec<String>,

    /// Node IDs to activate when this node **fails**.
    pub on_failure: Vec<String>,

    /// ID of a completed node whose output to use as this node's input.
    pub input_from: Option<String>,

    /// Template string with `{output}` placeholder replaced by `input_from`'s result.
    /// Ignored when `input_from` is `None`.
    pub input_template: Option<String>,

    /// Default input when neither `input_from` nor the initial input applies.
    pub base_input: Option<String>,
}

// ── Graph ─────────────────────────────────────────────────────────────────────

/// Builder that owns a collection of [`TaskNode`]s.
pub struct TaskGraph {
    pub(crate) nodes: Vec<TaskNode>,
}

impl TaskGraph {
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Append a node and return `self` for chaining.
    pub fn with_node(mut self, node: TaskNode) -> Self {
        self.nodes.push(node);
        self
    }

    fn validate(&self) -> Result<(), String> {
        let mut ids = HashSet::new();
        for node in &self.nodes {
            let id = node.id.trim();
            if id.is_empty() {
                return Err("task graph node id cannot be empty".to_string());
            }
            if !ids.insert(id.to_string()) {
                return Err(format!("task graph has duplicate node id '{}'", id));
            }
        }

        let id_set: HashSet<&str> = self.nodes.iter().map(|n| n.id.as_str()).collect();
        for node in &self.nodes {
            for dep in &node.depends_on {
                if !id_set.contains(dep.as_str()) {
                    return Err(format!(
                        "node '{}' depends on unknown node '{}'",
                        node.id, dep
                    ));
                }
            }
            for target in &node.on_success {
                if !id_set.contains(target.as_str()) {
                    return Err(format!(
                        "node '{}' on_success targets unknown node '{}'",
                        node.id, target
                    ));
                }
            }
            for target in &node.on_failure {
                if !id_set.contains(target.as_str()) {
                    return Err(format!(
                        "node '{}' on_failure targets unknown node '{}'",
                        node.id, target
                    ));
                }
            }
            if let Some(from) = &node.input_from
                && !id_set.contains(from.as_str())
            {
                return Err(format!(
                    "node '{}' input_from references unknown node '{}'",
                    node.id, from
                ));
            }
        }

        // DAG validation over dependency edges.
        let mut indegree: HashMap<&str, usize> = self
            .nodes
            .iter()
            .map(|n| (n.id.as_str(), n.depends_on.len()))
            .collect();
        let mut edges: HashMap<&str, Vec<&str>> = HashMap::new();
        for node in &self.nodes {
            for dep in &node.depends_on {
                edges
                    .entry(dep.as_str())
                    .or_default()
                    .push(node.id.as_str());
            }
        }

        let mut q: VecDeque<&str> = indegree
            .iter()
            .filter_map(|(id, degree)| (*degree == 0).then_some(*id))
            .collect();
        let mut visited = 0usize;
        while let Some(id) = q.pop_front() {
            visited += 1;
            if let Some(children) = edges.get(id) {
                for child in children {
                    let next = indegree
                        .get_mut(child)
                        .expect("validated node ids must exist in indegree map");
                    *next = next.saturating_sub(1);
                    if *next == 0 {
                        q.push_back(child);
                    }
                }
            }
        }
        if visited != self.nodes.len() {
            return Err("task graph dependency cycle detected".to_string());
        }

        Ok(())
    }

    /// Return nodes whose `depends_on` are all present in `completed`.
    ///
    /// Nodes already in `completed` or `failed` are excluded. Activation
    /// filtering (the `on_success` / `on_failure` reachability set) is the
    /// responsibility of the caller ([`TaskGraphRunner`]).
    pub fn ready_nodes<'a>(
        &'a self,
        completed: &HashMap<String, String>,
        failed: &HashSet<String>,
    ) -> Vec<&'a TaskNode> {
        self.nodes
            .iter()
            .filter(|node| {
                !completed.contains_key(&node.id)
                    && !failed.contains(&node.id)
                    && node
                        .depends_on
                        .iter()
                        .all(|dep| completed.contains_key(dep))
            })
            .collect()
    }

    /// Resolve the actual input string for a node.
    ///
    /// Priority: `input_from` + optional template → `base_input` → `initial`.
    pub fn resolve_input(
        &self,
        node: &TaskNode,
        completed: &HashMap<String, String>,
        initial: &str,
    ) -> String {
        if let Some(from_id) = &node.input_from {
            let output = completed.get(from_id).map(|s| s.as_str()).unwrap_or("");
            if let Some(template) = &node.input_template {
                template.replace("{output}", output)
            } else {
                output.to_string()
            }
        } else if let Some(base) = &node.base_input {
            base.clone()
        } else {
            initial.to_string()
        }
    }
}

impl Default for TaskGraph {
    fn default() -> Self {
        Self::new()
    }
}

// ── Result ────────────────────────────────────────────────────────────────────

/// Outcome of running a [`TaskGraph`].
pub enum TaskGraphResult {
    /// Every activated node completed successfully.
    AllSucceeded { outputs: HashMap<String, String> },
    /// At least one node failed; other nodes may have succeeded (or been skipped).
    PartialSuccess {
        outputs: HashMap<String, String>,
        failed: HashMap<String, String>,
    },
}

// ── Runner ────────────────────────────────────────────────────────────────────

/// Concurrent executor for a [`TaskGraph`].
///
/// Uses a semaphore to cap concurrency at `max_concurrent` (default 3) and
/// a [`JoinSet`] to wait on whichever task finishes next.
pub struct TaskGraphRunner {
    registry: Arc<SubagentRegistry>,
    max_concurrent: usize,
}

impl TaskGraphRunner {
    pub fn new(registry: Arc<SubagentRegistry>) -> Self {
        Self {
            registry,
            max_concurrent: 3,
        }
    }

    /// Override max in-flight nodes. Values < 1 are coerced to 1.
    pub fn with_max_concurrent(mut self, max: usize) -> Self {
        self.max_concurrent = max.max(1);
        self
    }

    /// Execute the graph.
    ///
    /// ## Scheduling algorithm
    ///
    /// 1. Activate all root nodes (those with no `depends_on`).
    /// 2. Repeatedly collect *ready + activated* nodes and spawn them (bounded by semaphore).
    /// 3. After each node finishes:
    ///    - **Success** → add its `on_success` targets to the activated set.
    ///    - **Failure** → add its `on_failure` targets to the activated set.
    /// 4. Repeat until nothing remains in flight.
    pub async fn run(
        &self,
        graph: TaskGraph,
        initial_input: &str,
        provider: Arc<dyn LlmProvider>,
        tool_executor: Option<Arc<dyn ToolExecutor>>,
    ) -> anyhow::Result<TaskGraphResult> {
        graph.validate().map_err(anyhow::Error::msg)?;

        let mut completed: HashMap<String, String> = HashMap::new();
        let mut failed: HashMap<String, String> = HashMap::new();
        // Set of node IDs eligible to run (reachable in the current execution path).
        let mut activated: HashSet<String> = HashSet::new();

        // Seed: root nodes are immediately eligible.
        for node in &graph.nodes {
            if node.depends_on.is_empty() {
                activated.insert(node.id.clone());
            }
        }

        let semaphore = Arc::new(Semaphore::new(self.max_concurrent));
        let mut join_set: JoinSet<(String, Result<String, String>)> = JoinSet::new();
        let mut in_flight: HashSet<String> = HashSet::new();

        loop {
            // Collect nodes that are ready to run: activated, not in-flight,
            // not already complete/failed, and all deps satisfied.
            let failed_ids: HashSet<String> = failed.keys().cloned().collect();
            let to_spawn: Vec<(String, SubagentConfig, String)> = graph
                .ready_nodes(&completed, &failed_ids)
                .into_iter()
                .filter(|n| activated.contains(&n.id) && !in_flight.contains(&n.id))
                .map(|n| {
                    let input = graph.resolve_input(n, &completed, initial_input);
                    (n.id.clone(), n.config.clone(), input)
                })
                .collect();

            // Spawn them, each holding a semaphore permit for the duration.
            for (node_id, config, input) in to_spawn {
                let permit = semaphore.clone().acquire_owned().await?;
                let registry = self.registry.clone();
                let provider = provider.clone();
                let executor = tool_executor.clone();

                in_flight.insert(node_id.clone());
                join_set.spawn(async move {
                    let _permit = permit; // released when this async block exits
                    let result = registry
                        .spawn_and_wait(config, provider, input, None, executor)
                        .await;
                    (node_id, result)
                });
            }

            // If nothing is running (and we couldn't spawn anything new), we're done.
            if join_set.is_empty() {
                break;
            }

            // Wait for the next task to complete.
            match join_set.join_next().await {
                Some(Ok((node_id, Ok(output)))) => {
                    in_flight.remove(&node_id);
                    // Activate conditional successors for the success path.
                    if let Some(node) = graph.nodes.iter().find(|n| n.id == node_id) {
                        for target in &node.on_success {
                            activated.insert(target.clone());
                        }
                    }
                    completed.insert(node_id, output);
                }
                Some(Ok((node_id, Err(error)))) => {
                    in_flight.remove(&node_id);
                    // Activate conditional successors for the failure path.
                    if let Some(node) = graph.nodes.iter().find(|n| n.id == node_id) {
                        for target in &node.on_failure {
                            activated.insert(target.clone());
                        }
                    }
                    failed.insert(node_id, error);
                }
                Some(Err(join_err)) => {
                    return Err(anyhow::anyhow!("[task_graph] join error: {}", join_err));
                }
                None => break, // join_set exhausted
            }
        }

        if failed.is_empty() {
            Ok(TaskGraphResult::AllSucceeded { outputs: completed })
        } else {
            Ok(TaskGraphResult::PartialSuccess {
                outputs: completed,
                failed,
            })
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::subagent::SubagentRegistry;

    fn mk_config(name: &str) -> SubagentConfig {
        SubagentConfig::new(name, "be helpful", "test-model", "test-provider")
    }

    fn node(id: &str, depends_on: Vec<&str>) -> TaskNode {
        TaskNode {
            id: id.to_string(),
            config: mk_config(id),
            depends_on: depends_on.into_iter().map(|s| s.to_string()).collect(),
            on_success: Vec::new(),
            on_failure: Vec::new(),
            input_from: None,
            input_template: None,
            base_input: None,
        }
    }

    #[test]
    fn ready_nodes_no_deps() {
        let graph = TaskGraph::new()
            .with_node(node("a", vec![]))
            .with_node(node("b", vec![]));
        let completed = HashMap::new();
        let failed = HashSet::new();
        let ready: Vec<&str> = graph
            .ready_nodes(&completed, &failed)
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        assert!(ready.contains(&"a"));
        assert!(ready.contains(&"b"));
    }

    #[test]
    fn ready_nodes_waits_for_dep() {
        let graph = TaskGraph::new()
            .with_node(node("a", vec![]))
            .with_node(node("b", vec!["a"]));
        let completed = HashMap::new();
        let failed = HashSet::new();
        let ready: Vec<&str> = graph
            .ready_nodes(&completed, &failed)
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        // "b" must wait for "a"
        assert!(ready.contains(&"a"));
        assert!(!ready.contains(&"b"));
    }

    #[test]
    fn ready_nodes_after_dep_complete() {
        let graph = TaskGraph::new()
            .with_node(node("a", vec![]))
            .with_node(node("b", vec!["a"]));
        let mut completed = HashMap::new();
        completed.insert("a".to_string(), "result-a".to_string());
        let failed = HashSet::new();
        let ready: Vec<&str> = graph
            .ready_nodes(&completed, &failed)
            .iter()
            .map(|n| n.id.as_str())
            .collect();
        assert!(!ready.contains(&"a")); // already completed
        assert!(ready.contains(&"b"));
    }

    #[test]
    fn ready_nodes_excludes_failed() {
        let graph = TaskGraph::new().with_node(node("a", vec![]));
        let completed = HashMap::new();
        let mut failed = HashSet::new();
        failed.insert("a".to_string());
        let ready = graph.ready_nodes(&completed, &failed);
        assert!(ready.is_empty());
    }

    #[test]
    fn resolve_input_from_completed() {
        let mut completed = HashMap::new();
        completed.insert("prev".to_string(), "the-output".to_string());

        let n = TaskNode {
            id: "n".to_string(),
            config: mk_config("n"),
            depends_on: vec![],
            on_success: vec![],
            on_failure: vec![],
            input_from: Some("prev".to_string()),
            input_template: Some("Result: {output}".to_string()),
            base_input: None,
        };
        let graph = TaskGraph::new().with_node(n);
        let result = graph.resolve_input(&graph.nodes[0], &completed, "initial");
        assert_eq!(result, "Result: the-output");
    }

    #[test]
    fn resolve_input_falls_back_to_base() {
        let n = TaskNode {
            id: "n".to_string(),
            config: mk_config("n"),
            depends_on: vec![],
            on_success: vec![],
            on_failure: vec![],
            input_from: None,
            input_template: None,
            base_input: Some("base-input".to_string()),
        };
        let graph = TaskGraph::new().with_node(n);
        let result = graph.resolve_input(&graph.nodes[0], &HashMap::new(), "initial");
        assert_eq!(result, "base-input");
    }

    #[test]
    fn resolve_input_falls_back_to_initial() {
        let n = TaskNode {
            id: "n".to_string(),
            config: mk_config("n"),
            depends_on: vec![],
            on_success: vec![],
            on_failure: vec![],
            input_from: None,
            input_template: None,
            base_input: None,
        };
        let graph = TaskGraph::new().with_node(n);
        let result = graph.resolve_input(&graph.nodes[0], &HashMap::new(), "from-initial");
        assert_eq!(result, "from-initial");
    }

    #[test]
    fn validate_rejects_duplicate_node_ids() {
        let graph = TaskGraph::new()
            .with_node(node("dup", vec![]))
            .with_node(node("dup", vec![]));
        assert!(graph.validate().is_err());
    }

    #[test]
    fn validate_rejects_unknown_dependency() {
        let graph = TaskGraph::new().with_node(node("a", vec!["missing"]));
        assert!(graph.validate().is_err());
    }

    #[test]
    fn validate_rejects_unknown_branch_target() {
        let mut n = node("a", vec![]);
        n.on_success.push("missing".to_string());
        let graph = TaskGraph::new().with_node(n);
        assert!(graph.validate().is_err());
    }

    #[test]
    fn validate_rejects_dependency_cycle() {
        let graph = TaskGraph::new()
            .with_node(node("a", vec!["b"]))
            .with_node(node("b", vec!["a"]));
        assert!(graph.validate().is_err());
    }

    #[test]
    fn runner_with_max_concurrent_coerces_minimum_one() {
        let registry = Arc::new(SubagentRegistry::new());
        let runner = TaskGraphRunner::new(registry).with_max_concurrent(0);
        assert_eq!(runner.max_concurrent, 1);
    }
}
