//! Turns raw `gc`/`bd` payloads into the normalized `model::*` views,
//! computing health signals along the way. This is the join point: bead
//! `assignee` (e.g. "gastown__polecat-cc-hc0y") matches an agent's
//! `runtime_session_name` exactly, which is how we know what a given agent
//! is actually working on right now.

use crate::health::{self, Thresholds};
use crate::model::{
    AgentView, BeadRef, BeadStage, CheckIssue, CheckReport, CityView, DagNode, DagView, Health,
    QueueSummary, QueueView, RigView,
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

pub fn city_view(adapters: &Adapters, raw: gc::StatusResult) -> CityView {
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
            // Suspended rigs have nothing useful to show and the extra
            // `bd status` call is wasted work -- skip it.
            let bead_summary = if r.suspended {
                None
            } else {
                adapters.bd.status(&r.path).ok().map(|s| QueueSummary {
                    total: s.summary.total_issues,
                    ready: s.summary.ready_issues,
                    in_progress: s.summary.in_progress_issues,
                    blocked: s.summary.blocked_issues,
                    deferred: s.summary.deferred_issues,
                    closed: s.summary.closed_issues,
                })
            };
            RigView {
                name: r.name.clone(),
                path: r.path.clone(),
                prefix: r.prefix.clone(),
                running,
                suspended: r.suspended,
                health,
                bead_summary,
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

/// Health-check verdict: city-wide signals always, plus one rig's
/// in-progress beads and agents if given. Only `Dead`/`Stale` count as
/// problems (see `health::is_problem`) -- this is meant to be a scriptable
/// pass/fail gate, not a report of every non-green thing.
pub fn check(
    adapters: &Adapters,
    city: Option<&str>,
    rig: Option<&str>,
    thresholds: &Thresholds,
) -> anyhow::Result<CheckReport> {
    let now = Utc::now();
    let mut issues = Vec::new();

    let cview = city_view(adapters, adapters.gc.status(city)?);
    if health::is_problem(cview.health) {
        let message = if cview.degraded {
            format!("city degraded: {}", cview.signals.join("; "))
        } else {
            "city unusable".to_string()
        };
        issues.push(CheckIssue {
            scope: "city".to_string(),
            health: cview.health,
            message,
        });
    }
    for rv in &cview.rigs {
        if health::is_problem(rv.health) {
            issues.push(CheckIssue {
                scope: format!("rig:{}", rv.name),
                health: rv.health,
                message: "rig is active but has no live agents".to_string(),
            });
        }
    }

    if let Some(rig_name) = rig {
        let resolved = adapters.resolve_rig(city, rig_name)?;
        let bd_status = adapters.bd.status(&resolved.path)?;
        let in_progress = adapters.bd.list(&resolved.path, "in_progress")?;
        let qview = queue_view(
            &resolved.name,
            &resolved.path,
            resolved.running,
            bd_status,
            in_progress.clone(),
            now,
            thresholds,
        );
        for b in &qview.in_progress {
            if health::is_problem(b.health) {
                issues.push(CheckIssue {
                    scope: format!("bead:{}", b.id),
                    health: b.health,
                    message: format!("in_progress but stale: {}", b.title),
                });
            }
        }

        let rig_status = adapters.gc.rig_status(city, rig_name)?;
        let views = agent_views(rig_status.agents, &in_progress, now, thresholds);
        for a in &views {
            if health::is_problem(a.health) {
                let message = if a.health == Health::Dead {
                    "agent is not running".to_string()
                } else {
                    "agent is running but stuck on stale work".to_string()
                };
                issues.push(CheckIssue {
                    scope: format!("agent:{}", a.qualified_name),
                    health: a.health,
                    message,
                });
            }
        }
    }

    Ok(CheckReport {
        ok: issues.is_empty(),
        issues,
    })
}
