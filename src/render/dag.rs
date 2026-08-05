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
use std::io::{self, Write};

/// Renders the `dag` view into `w`. See `render/agents.rs` for why this
/// takes a generic `impl Write` instead of printing directly.
pub fn render(view: &DagView, tick: Option<u64>, w: &mut impl Write) -> io::Result<()> {
    writeln!(w, "{} {}", view.rig_name.bold(), view.rig_path.dimmed())?;
    writeln!(w)?;

    if view.nodes.is_empty() {
        writeln!(w, "  (no beads)")?;
        return Ok(());
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
            w,
        )?;
    }

    writeln!(w)?;
    write!(
        w,
        "  legend:  {} pending",
        bead_stage_glyph(crate::model::BeadStage::Pending, None)
    )?;
    write!(
        w,
        "  {} active (flashes in watch)",
        bead_stage_glyph(crate::model::BeadStage::Active, tick.or(Some(0)))
    )?;
    write!(
        w,
        "  {} merged",
        bead_stage_glyph(crate::model::BeadStage::Merged, None)
    )?;
    writeln!(w)?;
    Ok(())
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
    w: &mut impl Write,
) -> io::Result<()> {
    let Some(node) = by_id.get(id) else {
        return Ok(());
    };
    writeln!(w, "{prefix}{connector}{}", format_node(node, tick))?;

    let child_prefix = format!("{prefix}{next_addition}");
    if let Some(kids) = children.get(id) {
        for (i, kid) in kids.iter().enumerate() {
            let is_last = i == kids.len() - 1;
            let conn = if is_last { "└── " } else { "├── " };
            let addition = if is_last { "    " } else { "│   " };
            walk(kid, by_id, children, &child_prefix, conn, addition, tick, w)?;
        }
    }
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BeadStage;

    fn render_to_string(view: &DagView) -> String {
        colored::control::set_override(false);
        let mut buf = Vec::new();
        render(view, None, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    fn node(
        id: &str,
        title: &str,
        parent: Option<&str>,
        stage: BeadStage,
        priority: Option<i64>,
        blocked_by: Vec<String>,
        landed_unmerged: bool,
    ) -> DagNode {
        DagNode {
            id: id.to_string(),
            title: title.to_string(),
            status: "open".to_string(),
            priority,
            assignee: None,
            parent: parent.map(str::to_string),
            blocked_by,
            age_secs: None,
            stage,
            landed_unmerged,
        }
    }

    #[test]
    fn renders_empty_dag_as_no_beads() {
        let view = DagView {
            rig_name: "luminate".to_string(),
            rig_path: "/repo/luminate".to_string(),
            nodes: vec![],
        };
        let out = render_to_string(&view);
        assert!(out.contains("luminate"));
        assert!(out.contains("/repo/luminate"));
        assert!(out.contains("(no beads)"));
    }

    #[test]
    fn renders_a_tree_of_epic_and_subtasks_in_order() {
        let view = DagView {
            rig_name: "luminate".to_string(),
            rig_path: "/repo/luminate".to_string(),
            nodes: vec![
                node(
                    "epic-1",
                    "the epic",
                    None,
                    BeadStage::Active,
                    Some(1),
                    vec![],
                    false,
                ),
                node(
                    "epic-1.1",
                    "first child",
                    Some("epic-1"),
                    BeadStage::Merged,
                    Some(2),
                    vec![],
                    false,
                ),
                node(
                    "epic-1.2",
                    "second child",
                    Some("epic-1"),
                    BeadStage::Pending,
                    None,
                    vec![],
                    false,
                ),
            ],
        };
        let out = render_to_string(&view);
        let epic_pos = out.find("epic-1 ").unwrap();
        let child1_pos = out.find("epic-1.1").unwrap();
        let child2_pos = out.find("epic-1.2").unwrap();
        assert!(epic_pos < child1_pos);
        assert!(child1_pos < child2_pos);
        // Last child gets the `└──` connector, not-last gets `├──`.
        assert!(out.contains("├── "));
        assert!(out.contains("└── "));
        assert!(out.contains("the epic"));
        assert!(out.contains("first child"));
        assert!(out.contains("second child"));
        assert!(out.contains("P1"));
        assert!(out.contains("--")); // no-priority node
    }

    #[test]
    fn orphaned_parent_reference_falls_back_to_root() {
        // A node whose `parent` points at an id not present in `nodes` is
        // treated as a root, not silently dropped.
        let view = DagView {
            rig_name: "luminate".to_string(),
            rig_path: "/repo/luminate".to_string(),
            nodes: vec![node(
                "orphan-1",
                "orphaned",
                Some("does-not-exist"),
                BeadStage::Pending,
                None,
                vec![],
                false,
            )],
        };
        let out = render_to_string(&view);
        assert!(out.contains("orphaned"));
    }

    #[test]
    fn landed_unmerged_flag_and_blocked_by_are_shown() {
        let view = DagView {
            rig_name: "luminate".to_string(),
            rig_path: "/repo/luminate".to_string(),
            nodes: vec![node(
                "b-1",
                "stuck bead",
                None,
                BeadStage::Active,
                Some(1),
                vec!["b-0".to_string()],
                true,
            )],
        };
        let out = render_to_string(&view);
        assert!(out.contains("[landed, not closed]"));
        assert!(out.contains("(blocked by b-0)"));
    }

    #[test]
    fn legend_lists_all_three_stages() {
        let view = DagView {
            rig_name: "luminate".to_string(),
            rig_path: "/repo/luminate".to_string(),
            nodes: vec![node(
                "b-1",
                "a bead",
                None,
                BeadStage::Pending,
                None,
                vec![],
                false,
            )],
        };
        let out = render_to_string(&view);
        assert!(out.contains("legend:"));
        assert!(out.contains("pending"));
        assert!(out.contains("active (flashes in watch)"));
        assert!(out.contains("merged"));
    }
}
