//! Git-graph-style DAG render for a rig's beads: a `tree`/`git log --graph`
//! style tree keyed off parent/child, with cross-tree "blocks" edges shown
//! as inline annotations rather than a full graph layout -- beads here are
//! overwhelmingly tree-shaped (epic -> subtasks) plus a handful of blocks
//! edges, so a real Sugiyama-style DAG layout would be solving a problem
//! this data mostly doesn't have.

use super::color::bead_stage_glyph;
use crate::model::{DagNode, DagView};
use colored::Colorize;
use std::collections::HashMap;

pub fn render(view: &DagView, tick: Option<u64>) {
    println!("{} {}", view.rig_name.bold(), view.rig_path.dimmed());
    println!();

    if view.nodes.is_empty() {
        println!("  (no beads)");
        return;
    }

    let by_id: HashMap<&str, &DagNode> = view.nodes.iter().map(|n| (n.id.as_str(), n)).collect();

    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut roots: Vec<&str> = Vec::new();
    for n in &view.nodes {
        match n.parent.as_deref() {
            Some(p) if by_id.contains_key(p) => {
                children.entry(p).or_default().push(n.id.as_str());
            }
            _ => roots.push(n.id.as_str()),
        }
    }

    for (i, root) in roots.iter().enumerate() {
        let is_last = i == roots.len() - 1;
        walk(
            root,
            &by_id,
            &children,
            "",
            "",
            if is_last { "    " } else { "│   " },
            tick,
        );
    }

    println!();
    print!("  legend:");
    print!(
        "  {} pending",
        bead_stage_glyph(crate::model::BeadStage::Pending, None)
    );
    print!(
        "  {} active (flashes in watch)",
        bead_stage_glyph(crate::model::BeadStage::Active, tick.or(Some(0)))
    );
    print!(
        "  {} merged",
        bead_stage_glyph(crate::model::BeadStage::Merged, None)
    );
    println!();
}

#[allow(clippy::too_many_arguments)]
fn walk(
    id: &str,
    by_id: &HashMap<&str, &DagNode>,
    children: &HashMap<&str, Vec<&str>>,
    prefix: &str,
    connector: &str,
    next_addition: &str,
    tick: Option<u64>,
) {
    let Some(node) = by_id.get(id) else { return };
    println!("{prefix}{connector}{}", format_node(node, tick));

    let child_prefix = format!("{prefix}{next_addition}");
    if let Some(kids) = children.get(id) {
        for (i, kid) in kids.iter().enumerate() {
            let is_last = i == kids.len() - 1;
            let conn = if is_last { "└── " } else { "├── " };
            let addition = if is_last { "    " } else { "│   " };
            walk(kid, by_id, children, &child_prefix, conn, addition, tick);
        }
    }
}

fn format_node(node: &DagNode, tick: Option<u64>) -> String {
    let glyph = bead_stage_glyph(node.stage, tick);
    let priority = node
        .priority
        .map(|p| format!("P{p}"))
        .unwrap_or_else(|| "--".to_string());

    let landed_flag = if node.landed_unmerged {
        format!(" {}", "[landed, not closed]".yellow().bold())
    } else {
        String::new()
    };

    let blocked = if node.blocked_by.is_empty() {
        String::new()
    } else {
        format!(
            " {}",
            format!("(blocked by {})", node.blocked_by.join(", ")).dimmed()
        )
    };

    format!(
        "{glyph} {} {:<3} {}{landed_flag}{blocked}",
        node.id.dimmed(),
        priority,
        node.title
    )
}
