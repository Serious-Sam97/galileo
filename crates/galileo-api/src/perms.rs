//! Permissions: what a member may do in a project. A role is a preset of permissions; a user can
//! be granted or denied single permissions on top of it, per organization (`member_permissions`).
//! The Master holds every permission everywhere.
//!
//! Seeing a project's data (traces, logs, metrics, boards) needs no permission: membership is
//! enough. Everything that changes something, or reveals personal data, needs one.

use std::collections::HashMap;

use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Perm {
    /// LLM prompts and completions, user e-mails and other personal attributes.
    ViewSensitive,
    /// Boards, saved queries, triggers, SLOs, maintenance windows, on-call, shares, annotations, deploys.
    EditContent,
    /// Resolve, ignore and reopen issues; acknowledge incidents.
    TriageIssues,
    /// AI gateway: providers, routes, budgets, prompts, datasets, evals.
    ManageAi,
    /// Ingest: API keys, redaction rules, log pipelines and metrics, sampling, quotas.
    ManageIngest,
    /// Project settings, notification channels, digests, issue settings, config import.
    ManageProject,
    /// Organization members' roles and project access.
    ManageMembers,
    /// Audit log and configuration export.
    AuditExport,
    /// Create and delete projects, organization settings.
    ManageOrg,
}

pub const ALL: [Perm; 9] = [
    Perm::ViewSensitive, Perm::EditContent, Perm::TriageIssues, Perm::ManageAi, Perm::ManageIngest,
    Perm::ManageProject, Perm::ManageMembers, Perm::AuditExport, Perm::ManageOrg,
];

impl Perm {
    pub fn key(self) -> &'static str {
        match self {
            Perm::ViewSensitive => "view_sensitive",
            Perm::EditContent => "edit_content",
            Perm::TriageIssues => "triage_issues",
            Perm::ManageAi => "manage_ai",
            Perm::ManageIngest => "manage_ingest",
            Perm::ManageProject => "manage_project",
            Perm::ManageMembers => "manage_members",
            Perm::AuditExport => "audit_export",
            Perm::ManageOrg => "manage_org",
        }
    }
    pub fn from_key(k: &str) -> Option<Perm> {
        ALL.iter().copied().find(|p| p.key() == k)
    }
    pub fn label(self) -> &'static str {
        match self {
            Perm::ViewSensitive => "See sensitive data",
            Perm::EditContent => "Edit boards, queries, triggers and SLOs",
            Perm::TriageIssues => "Triage issues",
            Perm::ManageAi => "Manage the AI gateway",
            Perm::ManageIngest => "Manage API keys and ingest",
            Perm::ManageProject => "Manage project settings",
            Perm::ManageMembers => "Manage members and roles",
            Perm::AuditExport => "See the audit log and export config",
            Perm::ManageOrg => "Create and delete projects",
        }
    }
    pub fn description(self) -> &'static str {
        match self {
            Perm::ViewSensitive => "LLM prompts and completions, user e-mails and other personal attributes.",
            Perm::EditContent => "Also maintenance windows, on-call, shares, annotations and deploy markers.",
            Perm::TriageIssues => "Resolve, ignore and reopen issues; acknowledge incidents.",
            Perm::ManageAi => "Providers, routes, budgets, prompts, datasets and evals.",
            Perm::ManageIngest => "API keys, redaction rules, log pipelines, sampling and quotas.",
            Perm::ManageProject => "Settings, notification channels, digests, issue settings, config import.",
            Perm::ManageMembers => "Change members' roles and project access. Creating accounts is the Master's.",
            Perm::AuditExport => "Read who changed what, and export the project configuration.",
            Perm::ManageOrg => "Create and delete projects and change organization settings.",
        }
    }
}

/// The permissions a role grants. Project roles (`viewer`, `editor`, `admin`) and organization roles
/// (`viewer`, `member`, `admin`, `owner`) share the scale; `member` is an editor.
pub fn preset(role: &str) -> &'static [Perm] {
    match role {
        "owner" => &ALL,
        "admin" => &[
            Perm::ViewSensitive, Perm::EditContent, Perm::TriageIssues, Perm::ManageAi, Perm::ManageIngest,
            Perm::ManageProject, Perm::ManageMembers, Perm::AuditExport,
        ],
        "editor" | "member" => &[Perm::ViewSensitive, Perm::EditContent, Perm::TriageIssues],
        _ => &[],
    }
}

/// A resolved set of permissions.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PermSet(u16);

impl PermSet {
    pub fn all() -> Self {
        Self::of(&ALL)
    }
    pub fn of(perms: &[Perm]) -> Self {
        PermSet(perms.iter().fold(0, |acc, p| acc | bit(*p)))
    }
    /// The role's preset with the user's overrides applied (`true` grants, `false` removes).
    pub fn resolve(role: &str, overrides: &HashMap<String, bool>) -> Self {
        let mut set = Self::of(preset(role));
        for (k, allow) in overrides {
            if let Some(p) = Perm::from_key(k) {
                if *allow { set.0 |= bit(p) } else { set.0 &= !bit(p) }
            }
        }
        set
    }
    pub fn has(self, p: Perm) -> bool {
        self.0 & bit(p) != 0
    }
    pub fn keys(self) -> Vec<&'static str> {
        ALL.iter().filter(|p| self.has(**p)).map(|p| p.key()).collect()
    }
}

fn bit(p: Perm) -> u16 {
    1 << (ALL.iter().position(|x| *x == p).expect("listed") as u16)
}

/// Attribute keys that only `ViewSensitive` may read. Responses for everyone else drop them.
pub const SENSITIVE_ATTRIBUTES: &[&str] = &[
    "gen_ai.prompt", "gen_ai.completion", "gen_ai.tool_calls", "gen_ai.input.messages", "gen_ai.output.messages",
    "user.email", "enduser.email", "user.name", "user.full_name",
];

pub fn is_sensitive(key: &str) -> bool {
    SENSITIVE_ATTRIBUTES.contains(&key)
}

/// Removes sensitive attributes, in place, from any JSON shape the API returns: objects keyed by
/// attribute name are pruned at every depth.
pub fn redact(v: &mut serde_json::Value) {
    match v {
        serde_json::Value::Object(m) => {
            m.retain(|k, _| !is_sensitive(k));
            for x in m.values_mut() { redact(x); }
        }
        serde_json::Value::Array(a) => a.iter_mut().for_each(redact),
        _ => {}
    }
}

/// Whether a query (as JSON) names a sensitive field anywhere a field goes: calculations, filters,
/// breakdowns, columns, orders, derived expressions, havings. Filter values and free-text search
/// are not fields and are not inspected.
pub fn query_mentions_sensitive(q: &serde_json::Value) -> bool {
    match q {
        serde_json::Value::Object(m) => m.iter().any(|(k, v)| k != "value" && k != "search" && query_mentions_sensitive(v)),
        serde_json::Value::Array(a) => a.iter().any(query_mentions_sensitive),
        serde_json::Value::String(s) => SENSITIVE_ATTRIBUTES.iter().any(|k| s == k || s.contains(&format!("({k})")) || s.contains(k)),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_nest() {
        let viewer = PermSet::of(preset("viewer"));
        let editor = PermSet::of(preset("editor"));
        let admin = PermSet::of(preset("admin"));
        let owner = PermSet::of(preset("owner"));
        for p in ALL {
            assert!(!viewer.has(p), "viewers only read");
            if editor.has(p) { assert!(admin.has(p)) }
            if admin.has(p) { assert!(owner.has(p)) }
        }
        assert_eq!(PermSet::of(preset("member")), editor);
        assert!(!admin.has(Perm::ManageOrg));
        assert_eq!(owner, PermSet::all());
    }

    #[test]
    fn overrides_grant_and_remove() {
        let o = HashMap::from([("manage_ai".to_string(), true), ("view_sensitive".to_string(), false), ("nonsense".to_string(), true)]);
        let s = PermSet::resolve("editor", &o);
        assert!(s.has(Perm::ManageAi));
        assert!(!s.has(Perm::ViewSensitive));
        assert!(s.has(Perm::EditContent));
        assert_eq!(s.keys().len(), 3);
    }

    #[test]
    fn redaction_prunes_at_every_depth() {
        let mut v = serde_json::json!({ "spans": [{ "attributes": { "gen_ai.prompt": "secret", "http.route": "/x", "user.email": "a@b" } }], "user.email": "c" });
        redact(&mut v);
        assert_eq!(v, serde_json::json!({ "spans": [{ "attributes": { "http.route": "/x" } }] }));
    }

    #[test]
    fn sensitive_fields_in_queries_are_found_where_fields_go() {
        let ok = serde_json::json!({ "dataset": "spans", "breakdowns": ["http_route"], "filters": [{ "field": "user.id", "op": "eq", "value": "user.email" }], "search": "gen_ai.prompt" });
        assert!(!query_mentions_sensitive(&ok), "values and search are not fields");
        for bad in [
            serde_json::json!({ "breakdowns": ["user.email"] }),
            serde_json::json!({ "columns": ["gen_ai.prompt"] }),
            serde_json::json!({ "filters": [{ "field": "gen_ai.completion", "op": "exists" }] }),
            serde_json::json!({ "derived": [{ "name": "x", "expr": "COUNT_DISTINCT(user.email) / 2" }] }),
        ] {
            assert!(query_mentions_sensitive(&bad), "{bad}");
        }
    }
}
