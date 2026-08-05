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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::mocks::{MockBd, MockGc, MockGit};
    use chrono::Duration;

    fn now() -> DateTime<Utc> {
        // Fixed instant (not Utc::now(), which the workflow harness forbids
        // in scripts and which would make "age" assertions non-deterministic
        // here regardless) -- every timestamp fixture below is relative to
        // this.
        DateTime::parse_from_rfc3339("2026-01-01T12:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
    }

    fn rfc3339(dt: DateTime<Utc>) -> String {
        dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
    }

    fn thresholds() -> Thresholds {
        Thresholds {
            stale_after_secs: 1800,
        }
    }

    fn bead(id: &str, status: &str) -> bd::BeadRaw {
        bd::BeadRaw {
            id: id.to_string(),
            title: format!("title for {id}"),
            status: status.to_string(),
            priority: Some(1),
            assignee: None,
            updated_at: None,
            started_at: None,
            parent: None,
            dependencies: vec![],
            metadata: serde_json::json!({}),
        }
    }

    fn city_status_raw() -> gc::StatusResult {
        gc::StatusResult {
            city_name: "testcity".to_string(),
            city_path: "/fake/testcity".to_string(),
            controller: gc::Controller {
                running: true,
                pid: Some(1),
                mode: Some("supervisor".to_string()),
                status: None,
            },
            running: true,
            suspended: false,
            health: gc::HealthBlock {
                usable: true,
                degraded: false,
                signals: vec![],
            },
            agents: vec![],
            rigs: vec![],
            summary: gc::SummaryBlock {
                total_agents: 0,
                running_agents: 0,
            },
            partial: false,
            partial_errors: vec![],
        }
    }

    fn rig_agent(
        qualified_name: &str,
        scope: &str,
        running: bool,
        suspended: bool,
    ) -> gc::AgentRaw {
        gc::AgentRaw {
            qualified_name: qualified_name.to_string(),
            scope: scope.to_string(),
            running,
            suspended,
        }
    }

    fn rig_raw(name: &str, suspended: bool) -> gc::RigRaw {
        gc::RigRaw {
            name: name.to_string(),
            path: format!("/fake/{name}"),
            prefix: None,
            suspended,
            running: None,
        }
    }

    // -- city_view -----------------------------------------------------

    #[test]
    fn city_view_marks_a_rig_healthy_only_with_a_live_non_suspended_rig_scoped_agent() {
        let mut raw = city_status_raw();
        raw.rigs = vec![rig_raw("alive", false), rig_raw("nobody-home", false)];
        raw.agents = vec![
            rig_agent("alive/gastown.polecat", "rig", true, false),
            // Same rig, but suspended -- shouldn't count as "live".
            rig_agent("alive/gastown.other", "rig", true, true),
            // Different rig ("nobody-home") has no matching agent at all.
            rig_agent("city-scope-thing", "city", true, false),
        ];

        let adapters = Adapters {
            gc: Box::new(MockGc::default()),
            bd: Box::new(MockBd {
                status: [
                    ("/fake/alive".to_string(), bd::StatusResult::default()),
                    ("/fake/nobody-home".to_string(), bd::StatusResult::default()),
                ]
                .into_iter()
                .collect(),
                ..Default::default()
            }),
            git: Box::new(MockGit::default()),
        };

        let view = city_view(&adapters, raw);
        let alive = view.rigs.iter().find(|r| r.name == "alive").unwrap();
        assert!(alive.running);
        assert_eq!(alive.health, Health::Healthy);

        let nobody_home = view.rigs.iter().find(|r| r.name == "nobody-home").unwrap();
        assert!(!nobody_home.running);
        assert_eq!(nobody_home.health, Health::Dead);
    }

    #[test]
    fn city_view_skips_bead_summary_for_suspended_rigs_but_fetches_it_for_active_ones() {
        let mut raw = city_status_raw();
        raw.rigs = vec![rig_raw("active", false), rig_raw("sleeping", true)];

        let adapters = Adapters {
            gc: Box::new(MockGc::default()),
            bd: Box::new(MockBd {
                status: [("/fake/active".to_string(), bd::StatusResult::default())]
                    .into_iter()
                    .collect(),
                ..Default::default()
            }),
            git: Box::new(MockGit::default()),
        };

        let view = city_view(&adapters, raw);
        let active = view.rigs.iter().find(|r| r.name == "active").unwrap();
        assert!(active.bead_summary.is_some());

        let sleeping = view.rigs.iter().find(|r| r.name == "sleeping").unwrap();
        assert_eq!(sleeping.health, Health::Suspended);
        assert!(
            sleeping.bead_summary.is_none(),
            "suspended rigs must never trigger the extra bd status call"
        );
    }

    #[test]
    fn city_view_overall_health_reflects_degraded_partial_and_healthy_cases() {
        let adapters = Adapters {
            gc: Box::new(MockGc::default()),
            bd: Box::new(MockBd::default()),
            git: Box::new(MockGit::default()),
        };

        let mut degraded = city_status_raw();
        degraded.health.degraded = true;
        assert_eq!(city_view(&adapters, degraded).health, Health::Dead);

        let mut unusable = city_status_raw();
        unusable.health.usable = false;
        assert_eq!(city_view(&adapters, unusable).health, Health::Dead);

        let mut partial = city_status_raw();
        partial.partial = true;
        assert_eq!(city_view(&adapters, partial).health, Health::Unknown);

        assert_eq!(
            city_view(&adapters, city_status_raw()).health,
            Health::Healthy
        );
    }

    // -- queue_view ------------------------------------------------------

    #[test]
    fn queue_view_health_is_the_worst_of_its_in_progress_beads() {
        let t = thresholds();
        let n = now();
        let mut fresh = bead("fresh-1", "in_progress");
        fresh.updated_at = Some(rfc3339(n - Duration::minutes(5)));
        let mut stale = bead("stale-1", "in_progress");
        stale.updated_at = Some(rfc3339(n - Duration::hours(2)));

        let view = queue_view(
            "rig",
            "/fake/rig",
            Some(true),
            bd::StatusResult::default(),
            vec![fresh, stale],
            n,
            &t,
        );
        assert_eq!(view.health, Health::Stale);
        assert_eq!(view.in_progress.len(), 2);
    }

    #[test]
    fn queue_view_is_healthy_when_all_in_progress_beads_are_fresh() {
        let t = thresholds();
        let n = now();
        let mut fresh = bead("fresh-1", "in_progress");
        fresh.updated_at = Some(rfc3339(n - Duration::minutes(1)));

        let view = queue_view(
            "rig",
            "/fake/rig",
            Some(true),
            bd::StatusResult::default(),
            vec![fresh],
            n,
            &t,
        );
        assert_eq!(view.health, Health::Healthy);
    }

    // -- agent_views -----------------------------------------------------

    fn rig_agent_raw(
        qualified_name: &str,
        runtime_session_name: Option<&str>,
        running: bool,
        suspended: bool,
        draining: bool,
    ) -> gc::RigAgentRaw {
        gc::RigAgentRaw {
            name: qualified_name.to_string(),
            qualified_name: qualified_name.to_string(),
            runtime_session_name: runtime_session_name.map(String::from),
            session_id: None,
            running,
            suspended,
            draining,
            status: None,
        }
    }

    #[test]
    fn agent_views_joins_current_bead_by_runtime_session_name() {
        let t = thresholds();
        let n = now();
        let mut in_progress_bead = bead("bead-1", "in_progress");
        in_progress_bead.assignee = Some("session-a".to_string());
        in_progress_bead.updated_at = Some(rfc3339(n - Duration::minutes(1)));

        let agents = vec![
            rig_agent_raw("rig/agent-a", Some("session-a"), true, false, false),
            rig_agent_raw("rig/agent-b", Some("session-b"), true, false, false),
            rig_agent_raw("rig/agent-c", None, true, false, false),
        ];

        let views = agent_views(agents, &[in_progress_bead], n, &t);

        let a = views
            .iter()
            .find(|v| v.qualified_name == "rig/agent-a")
            .unwrap();
        assert_eq!(a.current_bead.as_ref().unwrap().id, "bead-1");
        assert_eq!(a.health, Health::Healthy);

        let b = views
            .iter()
            .find(|v| v.qualified_name == "rig/agent-b")
            .unwrap();
        assert!(b.current_bead.is_none());
        assert_eq!(b.health, Health::Idle); // running, no assigned work

        let c = views
            .iter()
            .find(|v| v.qualified_name == "rig/agent-c")
            .unwrap();
        assert!(c.current_bead.is_none());
    }

    #[test]
    fn agent_views_health_priority_suspended_beats_dead_beats_work_state() {
        let t = thresholds();
        let n = now();

        let dead = agent_views(
            vec![rig_agent_raw("rig/a", None, false, false, false)],
            &[],
            n,
            &t,
        );
        assert_eq!(dead[0].health, Health::Dead);

        let suspended = agent_views(
            vec![rig_agent_raw("rig/a", None, false, true, false)],
            &[],
            n,
            &t,
        );
        assert_eq!(suspended[0].health, Health::Suspended);

        let draining = agent_views(
            vec![rig_agent_raw("rig/a", None, true, false, true)],
            &[],
            n,
            &t,
        );
        assert_eq!(draining[0].health, Health::Idle);
    }

    // -- dag_view ---------------------------------------------------------

    fn adapters_for_dag(
        beads_by_filter: Vec<(&str, &str, Vec<bd::BeadRaw>)>,
        git: MockGit,
    ) -> Adapters {
        let mut list = std::collections::HashMap::new();
        for (dir, filter, beads) in beads_by_filter {
            list.insert((dir.to_string(), filter.to_string()), beads);
        }
        Adapters {
            gc: Box::new(MockGc::default()),
            bd: Box::new(MockBd {
                list,
                ..Default::default()
            }),
            git: Box::new(git),
        }
    }

    #[test]
    fn dag_view_classifies_stage_by_status() {
        let rig = rig_raw("rig", false);
        let beads = vec![
            bead("open-1", "open"),
            bead("blocked-1", "blocked"),
            bead("deferred-1", "deferred"),
            bead("active-1", "in_progress"),
            bead("merged-1", "closed"),
        ];
        let adapters = adapters_for_dag(
            vec![(
                "/fake/rig",
                "open,in_progress,blocked,deferred,closed",
                beads,
            )],
            MockGit::default(),
        );

        let view = dag_view(&adapters, &rig, true, now()).unwrap();
        let stage_of = |id: &str| view.nodes.iter().find(|n| n.id == id).unwrap().stage;
        assert_eq!(stage_of("open-1"), BeadStage::Pending);
        assert_eq!(stage_of("blocked-1"), BeadStage::Pending);
        assert_eq!(stage_of("deferred-1"), BeadStage::Pending);
        assert_eq!(stage_of("active-1"), BeadStage::Active);
        assert_eq!(stage_of("merged-1"), BeadStage::Merged);
    }

    #[test]
    fn dag_view_excludes_closed_unless_include_closed() {
        let rig = rig_raw("rig", false);
        let beads = vec![bead("open-1", "open")];
        let adapters = adapters_for_dag(
            vec![("/fake/rig", "open,in_progress,blocked,deferred", beads)],
            MockGit::default(),
        );
        let view = dag_view(&adapters, &rig, false, now()).unwrap();
        assert_eq!(view.nodes.len(), 1);
    }

    #[test]
    fn dag_view_extracts_blocked_by_from_blocks_dependencies_only() {
        let rig = rig_raw("rig", false);
        let mut blocked = bead("blocked-1", "blocked");
        blocked.dependencies = vec![
            bd::DependencyRaw {
                depends_on_id: "parent-1".to_string(),
                dep_type: "parent-child".to_string(),
            },
            bd::DependencyRaw {
                depends_on_id: "blocker-1".to_string(),
                dep_type: "blocks".to_string(),
            },
        ];
        let adapters = adapters_for_dag(
            vec![(
                "/fake/rig",
                "open,in_progress,blocked,deferred",
                vec![blocked],
            )],
            MockGit::default(),
        );
        let view = dag_view(&adapters, &rig, false, now()).unwrap();
        assert_eq!(view.nodes[0].blocked_by, vec!["blocker-1"]);
    }

    #[test]
    fn dag_view_landed_unmerged_requires_branch_and_target_and_git_confirmation() {
        let rig = rig_raw("rig", false);

        let mut has_both_and_merged = bead("landed-1", "in_progress");
        has_both_and_merged.metadata =
            serde_json::json!({"branch": "polecat/landed-1", "gc.work_branch": "master"});

        let mut has_both_not_merged = bead("stuck-1", "in_progress");
        has_both_not_merged.metadata =
            serde_json::json!({"branch": "polecat/stuck-1", "gc.work_branch": "master"});

        let mut no_branch_metadata = bead("direct-commit-1", "in_progress");
        no_branch_metadata.metadata = serde_json::json!({});

        let mut git = MockGit::default();
        git.merged.insert(
            (
                "/fake/rig".to_string(),
                "polecat/landed-1".to_string(),
                "master".to_string(),
            ),
            true,
        );

        let adapters = adapters_for_dag(
            vec![(
                "/fake/rig",
                "open,in_progress,blocked,deferred",
                vec![has_both_and_merged, has_both_not_merged, no_branch_metadata],
            )],
            git,
        );
        let view = dag_view(&adapters, &rig, false, now()).unwrap();

        let by_id = |id: &str| view.nodes.iter().find(|n| n.id == id).unwrap();
        assert!(by_id("landed-1").landed_unmerged);
        assert!(!by_id("stuck-1").landed_unmerged);
        assert!(
            !by_id("direct-commit-1").landed_unmerged,
            "no branch metadata (direct-commit model) must never claim landed_unmerged"
        );
    }

    // -- check -------------------------------------------------------------

    fn adapters_for_check(
        city_status: gc::StatusResult,
        rig_list: Vec<gc::RigRaw>,
        bd_status: bd::StatusResult,
        in_progress: Vec<bd::BeadRaw>,
        rig_agents: Vec<gc::RigAgentRaw>,
    ) -> Adapters {
        let rig_path = rig_list.first().map(|r| r.path.clone()).unwrap_or_default();
        Adapters {
            gc: Box::new(MockGc {
                status: Some(city_status),
                rig_list: Some(gc::RigListResult {
                    rigs: rig_list.clone(),
                }),
                rig_status: rig_list
                    .into_iter()
                    .map(|r| {
                        (
                            r.name.clone(),
                            gc::RigStatusResult {
                                agents: rig_agents.clone(),
                                rig: r,
                            },
                        )
                    })
                    .collect(),
            }),
            bd: Box::new(MockBd {
                status: [(rig_path.clone(), bd_status)].into_iter().collect(),
                list: [((rig_path, "in_progress".to_string()), in_progress)]
                    .into_iter()
                    .collect(),
            }),
            git: Box::new(MockGit::default()),
        }
    }

    #[test]
    fn check_is_ok_when_everything_is_healthy() {
        let adapters = adapters_for_check(
            city_status_raw(),
            vec![],
            bd::StatusResult::default(),
            vec![],
            vec![],
        );
        let report = check(&adapters, None, None, &thresholds()).unwrap();
        assert!(report.ok);
        assert!(report.issues.is_empty());
    }

    #[test]
    fn check_flags_a_degraded_city() {
        let mut raw = city_status_raw();
        raw.health.degraded = true;
        raw.health.signals = vec!["dolt is unhappy".to_string()];
        let adapters = adapters_for_check(raw, vec![], bd::StatusResult::default(), vec![], vec![]);
        let report = check(&adapters, None, None, &thresholds()).unwrap();
        assert!(!report.ok);
        assert!(report.issues[0].message.contains("dolt is unhappy"));
    }

    #[test]
    fn check_flags_a_stale_in_progress_bead_and_a_dead_agent_for_the_given_rig() {
        let mut stale_bead = bead("stale-1", "in_progress");
        stale_bead.updated_at = Some(rfc3339(now() - Duration::hours(2)));
        stale_bead.assignee = Some("session-a".to_string());

        let dead_agent = rig_agent_raw("rig/dead-agent", None, false, false, false);

        let adapters = adapters_for_check(
            city_status_raw(),
            vec![rig_raw("rig", false)],
            bd::StatusResult::default(),
            vec![stale_bead],
            vec![dead_agent],
        );
        let report = check(&adapters, None, Some("rig"), &thresholds()).unwrap();
        assert!(!report.ok);
        assert!(report.issues.iter().any(|i| i.scope == "bead:stale-1"));
        assert!(report
            .issues
            .iter()
            .any(|i| i.scope == "agent:rig/dead-agent" && i.message.contains("not running")));
    }
}
