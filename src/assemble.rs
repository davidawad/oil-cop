//! Turns raw `gc`/`bd` payloads into the normalized `model::*` views,
//! computing health signals along the way. This is the join point: bead
//! `assignee` (e.g. "gastown__polecat-cc-hc0y") matches an agent's
//! `runtime_session_name` exactly, which is how we know what a given agent
//! is actually working on right now.

use crate::health::{self, Thresholds};
use crate::model::{
    AgentView, BeadRef, BeadStage, CityView, DagNode, DagView, Health, QueueSummary, QueueView,
    RigView,
};
use crate::sources::adapters::Adapters;
use crate::sources::{bd, gc};
use chrono::{DateTime, Utc};

pub fn bead_ref(raw: &bd::BeadRaw, now: DateTime<Utc>, thresholds: &Thresholds) -> BeadRef {
    let age = health::age_secs(raw.updated_at.as_deref(), now);
    BeadRef {
        id: raw.id.clone(),
        title: raw.title.clone(),
        status: raw.status.clone(),
        priority: raw.priority,
        assignee: raw.assignee.clone(),
        started_at: raw.started_at.clone(),
        updated_at: raw.updated_at.clone(),
        age_secs: age,
        health: health::bead_health(&raw.status, age, thresholds),
    }
}

pub fn city_view(raw: gc::StatusResult) -> CityView {
    // `gc status`'s top-level rigs array doesn't carry a `running` flag
    // (that's only on `gc rig list`) -- but the agents array does, scoped by
    // a "<rig-name>/<agent>" qualified_name for rig-scoped agents. Roll that
    // up into "does this rig have any live, non-suspended agent right now."
    let rig_has_live_agent = |rig_name: &str| -> bool {
        raw.agents.iter().any(|a| {
            a.scope == "rig"
                && a.running
                && !a.suspended
                && a.qualified_name
                    .split_once('/')
                    .map(|(prefix, _)| prefix == rig_name)
                    .unwrap_or(false)
        })
    };

    let rigs: Vec<RigView> = raw
        .rigs
        .iter()
        .map(|r| {
            let running = rig_has_live_agent(&r.name);
            let health = if r.suspended {
                Health::Suspended
            } else if running {
                Health::Healthy
            } else {
                Health::Dead
            };
            RigView {
                name: r.name.clone(),
                path: r.path.clone(),
                prefix: r.prefix.clone(),
                running,
                suspended: r.suspended,
                health,
            }
        })
        .collect();

    let overall = if raw.health.degraded || !raw.health.usable {
        Health::Dead
    } else if raw.partial {
        Health::Unknown
    } else {
        Health::Healthy
    };

    CityView {
        city_name: raw.city_name,
        city_path: raw.city_path,
        city_running: raw.running,
        controller_running: raw.controller.running,
        controller_pid: raw.controller.pid,
        controller_mode: raw.controller.mode,
        controller_status: raw.controller.status,
        suspended: raw.suspended,
        usable: raw.health.usable,
        degraded: raw.health.degraded,
        signals: raw.health.signals,
        partial: raw.partial,
        partial_errors: raw.partial_errors,
        rigs,
        total_agents: raw.summary.total_agents.max(0) as usize,
        running_agents: raw.summary.running_agents.max(0) as usize,
        health: overall,
    }
}

/// Build the queue view for a rig from `bd status` + the in-progress bead
/// list (the only list we need in full detail; other buckets are counts).
pub fn queue_view(
    rig_name: &str,
    rig_path: &str,
    rig_running: Option<bool>,
    status: bd::StatusResult,
    in_progress_raw: Vec<bd::BeadRaw>,
    now: DateTime<Utc>,
    thresholds: &Thresholds,
) -> QueueView {
    let in_progress: Vec<BeadRef> = in_progress_raw
        .iter()
        .map(|b| bead_ref(b, now, thresholds))
        .collect();

    let health = health::worst_of(in_progress.iter().map(|b| b.health));

    QueueView {
        rig_name: rig_name.to_string(),
        rig_path: rig_path.to_string(),
        rig_running,
        summary: QueueSummary {
            total: status.summary.total_issues,
            ready: status.summary.ready_issues,
            in_progress: status.summary.in_progress_issues,
            blocked: status.summary.blocked_issues,
            deferred: status.summary.deferred_issues,
            closed: status.summary.closed_issues,
        },
        in_progress,
        health,
    }
}

/// Join `gc rig status` agents against in-progress beads by
/// `assignee == runtime_session_name` to show what each agent is doing.
pub fn agent_views(
    agents_raw: Vec<gc::RigAgentRaw>,
    in_progress_raw: &[bd::BeadRaw],
    now: DateTime<Utc>,
    thresholds: &Thresholds,
) -> Vec<AgentView> {
    agents_raw
        .into_iter()
        .map(|a| {
            let current_bead = a.runtime_session_name.as_deref().and_then(|session| {
                in_progress_raw
                    .iter()
                    .find(|b| b.assignee.as_deref() == Some(session))
                    .map(|b| bead_ref(b, now, thresholds))
            });

            let has_fresh_work = current_bead.as_ref().map(|b| b.health == Health::Healthy);
            let health = health::agent_health(a.running, a.suspended, a.draining, has_fresh_work);

            AgentView {
                name: a.name,
                qualified_name: a.qualified_name,
                scope: "rig".to_string(),
                running: a.running,
                suspended: a.suspended,
                draining: a.draining,
                raw_status: a.status,
                session_id: a.session_id,
                runtime_session_name: a.runtime_session_name,
                current_bead,
                health,
            }
        })
        .collect()
}

/// Build the DAG view for a rig: every non-closed bead (plus closed ones
/// too, if `include_closed`) as a node, tagged with its lifecycle stage and
/// whether it's "landed but not closed" -- an in_progress bead whose branch
/// has already merged into its target in git, the refinery-stuck signal.
pub fn dag_view(
    adapters: &Adapters,
    rig: &gc::RigRaw,
    include_closed: bool,
    now: DateTime<Utc>,
) -> anyhow::Result<DagView> {
    let statuses = if include_closed {
        "open,in_progress,blocked,deferred,closed"
    } else {
        "open,in_progress,blocked,deferred"
    };
    let beads = adapters.bd.list(&rig.path, statuses)?;

    let nodes = beads
        .iter()
        .map(|b| {
            let age = health::age_secs(b.updated_at.as_deref(), now);
            let stage = match b.status.as_str() {
                "closed" => BeadStage::Merged,
                "in_progress" => BeadStage::Active,
                _ => BeadStage::Pending,
            };
            let landed_unmerged = stage == BeadStage::Active
                && match (b.branch(), b.work_branch()) {
                    (Some(branch), Some(target)) => {
                        adapters.git.is_merged(&rig.path, branch, target)
                    }
                    _ => false,
                };
            DagNode {
                id: b.id.clone(),
                title: b.title.clone(),
                status: b.status.clone(),
                priority: b.priority,
                assignee: b.assignee.clone(),
                parent: b.parent.clone(),
                blocked_by: b.blocked_by().into_iter().map(String::from).collect(),
                age_secs: age,
                stage,
                landed_unmerged,
            }
        })
        .collect();

    Ok(DagView {
        rig_name: rig.name.clone(),
        rig_path: rig.path.clone(),
        nodes,
    })
}
