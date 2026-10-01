use axum::routing::{delete, get, post, put};
use axum::Router;

use crate::state::AppState;

pub mod alerts;
pub mod agents;
pub mod api_keys;
pub mod assistant;
pub mod auth;
pub mod boards;
pub mod channels;
pub mod gateway;
pub mod issues;
pub mod logs;
pub mod org;
pub mod search;
pub mod openapi;
pub mod sso;
pub mod members;
pub mod admin;
pub mod config_bundle;
pub mod projects;
pub mod query;
pub mod sessions;
pub mod shares;
pub mod rules;
pub mod saved_queries;
pub mod system;

pub fn api_router() -> Router<AppState> {
    Router::new()
        // auth
        .route("/auth/register", post(auth::register))
        .route("/auth/login", post(auth::login))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/setup", get(auth::setup_status))
        .route("/auth/password", post(auth::change_password))
        .route("/auth/sessions", get(auth::sessions))
        .route("/auth/sessions/{id}", delete(auth::revoke_session))
        // accounts (Master only)
        .route("/admin/users", get(admin::list_users).post(admin::create_user))
        .route("/admin/catalog", get(admin::catalog))
        .route("/admin/users/{user_id}", axum::routing::patch(admin::patch_user).delete(admin::delete_user))
        .route("/admin/users/{user_id}/reset-password", post(admin::reset_password))
        .route("/admin/users/{user_id}/orgs/{org_id}", put(admin::set_org_role))
        .route("/admin/users/{user_id}/projects/{project_id}", put(admin::set_project_role))
        .route("/admin/users/{user_id}/permissions", get(admin::get_permissions).put(admin::put_permissions))
        .route("/auth/oidc", get(sso::oidc_config))
        .route("/auth/oidc/start", get(sso::oidc_start))
        .route("/auth/oidc/callback", get(sso::oidc_callback))
        .route("/auth/2fa", get(sso::twofa_status))
        .route("/auth/2fa/setup", post(sso::twofa_setup))
        .route("/auth/2fa/enable", post(sso::twofa_enable))
        .route("/auth/2fa/disable", post(sso::twofa_disable))
        .route("/auth/2fa/verify", post(sso::twofa_verify))
        .route("/openapi.json", get(openapi::spec))
        .route("/docs", get(openapi::docs))
        // orgs + projects
        .route("/orgs", get(projects::list_orgs).post(projects::create_org))
        .route("/orgs/{org_id}/overview", get(org::overview))
        .route("/orgs/{org_id}/members", get(org::members))
        .route("/projects/{project_id}/members", get(members::list))
        .route("/projects/{project_id}/search", get(search::search))
        .route("/projects/{project_id}/members/{user_id}", put(members::set_role).delete(members::clear_role))
        .route("/projects/{project_id}/export", get(config_bundle::export))
        .route("/projects/{project_id}/import", post(config_bundle::import))
        .route("/orgs/{org_id}/assistant", get(org::get_assistant).put(org::put_assistant))
        .route("/orgs/{org_id}/members/{user_id}", axum::routing::patch(org::set_role).delete(org::remove_member))
        .route("/orgs/{org_id}/invites", get(org::list_invites).post(org::create_invite))
        .route("/orgs/{org_id}/invites/{invite_id}", delete(org::revoke_invite))
        .route("/orgs/{org_id}/audit", get(org::org_audit))
        .route("/auth/invite/{token}", get(org::invite_info))
        .route("/auth/invite/{token}/accept", post(org::invite_accept))
        .route("/auth/tokens", get(org::list_tokens).post(org::create_token))
        .route("/auth/tokens/{token_id}", delete(org::revoke_token))
        .route("/projects/{project_id}/audit", get(org::project_audit))
        .route("/projects/{project_id}/settings", get(org::get_project_settings).put(org::put_project_settings))
        .route("/projects", get(projects::list).post(projects::create))
        .route("/projects/{project_id}", get(projects::get).patch(projects::update).delete(projects::delete))
        // api keys
        .route("/projects/{project_id}/api-keys", get(api_keys::list).post(api_keys::create))
        .route("/projects/{project_id}/api-keys/{key_id}", delete(api_keys::revoke))
        // redaction rules
        .route("/projects/{project_id}/redaction-rules", get(rules::list).post(rules::create))
        .route("/projects/{project_id}/redaction-rules/{rule_id}", delete(rules::delete))
        .route("/projects/{project_id}/redaction-rules/test", post(rules::test_rules))
        // saved queries
        .route("/projects/{project_id}/saved-queries", get(saved_queries::list).post(saved_queries::create))
        .route(
            "/projects/{project_id}/saved-queries/{query_id}",
            get(saved_queries::get).put(saved_queries::update).delete(saved_queries::delete),
        )
        // boards
        .route("/projects/{project_id}/boards", get(boards::list).post(boards::create))
        .route("/projects/{project_id}/boards/{board_id}", get(boards::get).put(boards::update).delete(boards::delete))
        // event data
        .route("/projects/{project_id}/query", post(query::run))
        .route("/projects/{project_id}/query/history", get(boards::query_history))
        .route("/projects/{project_id}/incidents", get(alerts::list_incidents))
        .route("/projects/{project_id}/triggers/{trigger_id}/incidents", get(alerts::trigger_incidents))
        .route("/projects/{project_id}/triggers/{trigger_id}/incidents/{incident_id}/ack", post(alerts::ack_incident))
        .route("/projects/{project_id}/triggers/{trigger_id}/mute-group", post(alerts::mute_group))
        .route("/projects/{project_id}/maintenance-windows", get(alerts::list_windows).post(alerts::create_window))
        .route("/projects/{project_id}/maintenance-windows/{window_id}", delete(alerts::delete_window))
        .route("/projects/{project_id}/oncall", get(alerts::list_oncall).post(alerts::create_oncall))
        .route("/projects/{project_id}/oncall/{schedule_id}", axum::routing::put(alerts::update_oncall).delete(alerts::delete_oncall))
        .route("/ack/{token}", get(alerts::ack_by_token).post(alerts::ack_by_token_confirm))
        .route("/projects/{project_id}/log-pipeline", get(logs::get_pipeline).put(logs::put_pipeline))
        .route("/projects/{project_id}/log-pipeline/preview", post(logs::preview))
        .route("/projects/{project_id}/log-metrics", get(logs::list_metrics).post(logs::create_metric))
        .route("/projects/{project_id}/log-metrics/{metric_id}", delete(logs::delete_metric))
        .route("/projects/{project_id}/usage", get(logs::usage))
        .route("/projects/{project_id}/quotas", axum::routing::put(logs::put_quotas))
        .route("/projects/{project_id}/boards/templates", post(boards::create_template))
        .route("/projects/{project_id}/boards/{board_id}/settings", axum::routing::patch(boards::patch_settings))
        .route("/projects/{project_id}/boards/{board_id}/variables/{name}/values", get(boards::variable_values))
        .route("/projects/{project_id}/annotations", get(boards::list_annotations).post(boards::create_annotation))
        .route("/projects/{project_id}/annotations/{annotation_id}", delete(boards::delete_annotation))
        .route("/projects/{project_id}/channels/{channel_id}/send-image", post(boards::send_image))
        .route("/projects/{project_id}/traces", post(query::recent_traces))
        .route("/projects/{project_id}/traces/{trace_id}", get(query::trace))
        .route("/projects/{project_id}/bubbleup", post(query::bubbleup))
        .route("/projects/{project_id}/fields", get(query::fields))
        .route("/projects/{project_id}/fields/values", get(query::values))
        .route("/projects/{project_id}/metrics/names", get(query::metric_names))
        .route("/projects/{project_id}/services", get(query::services))
        .route("/projects/{project_id}/service-map", get(query::service_map))
        .route("/projects/{project_id}/query/parse", post(query::parse_dsl))
        .route("/projects/{project_id}/query/stringify", post(query::stringify_dsl))
        .route("/projects/{project_id}/query/export", post(query::export_csv))
        .route("/projects/{project_id}/agent-runs", get(agents::list))
        .route("/projects/{project_id}/agent-runs/{conversation_id}", get(agents::get))
        .route("/projects/{project_id}/assistant/chat", post(assistant::chat))
        .route("/projects/{project_id}/assistant/investigate", post(assistant::investigate))
        .route("/projects/{project_id}/assistant/explain-trace", post(assistant::explain_trace))
        .route("/projects/{project_id}/shares", post(shares::create))
        .route("/projects/{project_id}/shares/{slug}", get(shares::get))
        .route("/projects/{project_id}/db/nplusone", get(query::nplusone))
        .route("/projects/{project_id}/db/callers", get(query::callers))
        .route("/projects/{project_id}/users/{user_id}/timeline", get(query::user_timeline))
        .route("/projects/{project_id}/sessions", get(sessions::list))
        .route("/projects/{project_id}/sessions/{session_id}", get(sessions::get))
        .route("/projects/{project_id}/rum/vitals", get(sessions::vitals))
        // gateway config
        .route("/projects/{project_id}/gateway/providers", get(gateway::list_providers).post(gateway::create_provider))
        .route("/projects/{project_id}/gateway/providers/{provider_id}", axum::routing::put(gateway::update_provider).delete(gateway::delete_provider))
        .route("/projects/{project_id}/gateway/providers/{provider_id}/models", get(gateway::provider_models))
        .route("/projects/{project_id}/gateway/routes", get(gateway::list_routes).post(gateway::create_route))
        .route("/projects/{project_id}/gateway/routes/{route_id}", axum::routing::put(gateway::update_route).delete(gateway::delete_route))
        .route("/projects/{project_id}/gateway/prompts", get(gateway::list_prompts).post(gateway::create_prompt))
        .route("/projects/{project_id}/gateway/prompts/{prompt_id}", get(gateway::get_prompt).delete(gateway::delete_prompt))
        .route("/projects/{project_id}/gateway/prompts/{prompt_id}/versions", post(gateway::add_prompt_version))
        .route("/projects/{project_id}/gateway/prompts/{prompt_id}/promote", post(gateway::promote_prompt))
        .route("/projects/{project_id}/gateway/prompts/{prompt_id}/ci", post(gateway::run_ci).patch(gateway::patch_prompt))
        .route("/projects/{project_id}/gateway/datasets", get(gateway::list_datasets).post(gateway::create_dataset))
        .route("/projects/{project_id}/gateway/datasets/{dataset_id}", get(gateway::get_dataset).delete(gateway::delete_dataset))
        .route("/projects/{project_id}/gateway/datasets/{dataset_id}/items", post(gateway::add_items))
        .route("/projects/{project_id}/gateway/datasets/{dataset_id}/items/{item_id}", delete(gateway::delete_item))
        .route("/projects/{project_id}/gateway/usage", get(gateway::usage))
        .route("/projects/{project_id}/gateway/feedback", post(gateway::feedback))
        .route("/projects/{project_id}/gateway/quality", post(gateway::quality_for_spans))
        .route("/projects/{project_id}/gateway/evals", get(gateway::list_evals))
        .route("/projects/{project_id}/gateway/evals/run", post(gateway::run_eval))
        // notification channels + digest
        .route("/projects/{project_id}/channels", get(channels::list).post(channels::create))
        .route("/projects/{project_id}/channels/{channel_id}", axum::routing::put(channels::update).delete(channels::delete))
        .route("/projects/{project_id}/channels/{channel_id}/test", post(channels::test))
        .route("/projects/{project_id}/digest", get(channels::get_digest).put(channels::put_digest))
        .route("/projects/{project_id}/digest/send", post(channels::send_digest))
        .route("/projects/{project_id}/digest/preview", get(channels::preview_digest))
        // alerts
        .route("/projects/{project_id}/triggers", get(alerts::list_triggers).post(alerts::create_trigger))
        .route("/projects/{project_id}/triggers/preview", post(alerts::preview_trigger))
        .route("/projects/{project_id}/triggers/{trigger_id}", get(alerts::get_trigger).put(alerts::update_trigger).delete(alerts::delete_trigger))
        .route("/projects/{project_id}/triggers/{trigger_id}/test", post(alerts::test_trigger))
        .route("/projects/{project_id}/slos", get(alerts::list_slos).post(alerts::create_slo))
        .route("/projects/{project_id}/slos/{slo_id}", get(alerts::get_slo).put(alerts::update_slo).delete(alerts::delete_slo))
        // issues + deploys
        .route("/projects/{project_id}/issues", get(issues::list))
        .route("/projects/{project_id}/issues/{issue_id}", get(issues::get))
        .route("/projects/{project_id}/issues/{issue_id}/resolve", post(issues::resolve))
        .route("/projects/{project_id}/issues/{issue_id}/ignore", post(issues::ignore))
        .route("/projects/{project_id}/issues/{issue_id}/reopen", post(issues::reopen))
        .route("/projects/{project_id}/issue-settings", get(issues::get_settings).put(issues::put_settings))
        .route("/projects/{project_id}/deploys", get(issues::list_deploys).post(issues::create_deploy))
        // system
        .route("/system/health", get(system::health))
        .route("/system/stats", get(system::stats))
        .route("/system/detail", get(system::detail))
}
