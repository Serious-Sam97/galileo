//! Accounts end to end through the HTTP router: first-user setup, accounts created by the Master
//! with a temporary password, the forced change, permissions and overrides, resets, disabling,
//! lockout. Needs a throwaway Postgres: `GALILEO_TEST_PG=postgres://… cargo test -p galileo-api
//! --test accounts`. Skipped (passes) without it.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use axum::Router;
use serde_json::{json, Value};
use tower::ServiceExt;

use galileo_api::AppState;

async fn app() -> Option<(Router, sqlx::PgPool)> {
    let url = std::env::var("GALILEO_TEST_PG").ok()?;
    let pg = sqlx::postgres::PgPoolOptions::new().max_connections(5).connect(&url).await.expect("test postgres");
    for stmt in ["DROP SCHEMA public CASCADE", "CREATE SCHEMA public"] {
        sqlx::query(stmt).execute(&pg).await.expect("reset schema");
    }
    galileo_api::migrate(&pg).await.expect("migrate");
    let config = Arc::new(galileo_core::Config::default());
    // ClickHouse is never reached by these endpoints; the storage only has to exist.
    let storage: galileo_storage::DynStorage = Arc::new(galileo_storage::clickhouse::ClickHouseStorage::new(&config.clickhouse));
    let resolver = Arc::new(galileo_api::PgResolver::new(pg.clone()));
    let dyn_resolver: Arc<dyn galileo_otlp::ApiKeyResolver> = resolver.clone();
    let writer = galileo_otlp::writer::BatchWriter::start(storage.clone(), &config.ingest);
    let secret = [1u8; 32];
    let state = AppState {
        config: config.clone(),
        pg: pg.clone(),
        storage: storage.clone(),
        resolver,
        secret,
        ingest_stats: writer.handle().stats.clone(),
        gateway: galileo_gateway::Gateway::new(pg.clone(), storage.clone(), dyn_resolver, writer.handle(), secret),
        alerts: galileo_alerts::Evaluator::new(pg.clone(), storage, config.smtp.clone(), config.public_url.clone()),
        started_at: std::time::Instant::now(),
    };
    std::mem::forget(writer);
    Some((galileo_api::router(state), pg))
}

/// One request: (status, JSON body, session cookie set by the response).
async fn call(app: &Router, method: &str, path: &str, session: Option<&str>, body: Option<Value>) -> (StatusCode, Value, Option<String>) {
    let mut req = Request::builder().method(method).uri(path).header("content-type", "application/json");
    if let Some(s) = session {
        req = req.header("cookie", format!("galileo_session={s}"));
    }
    let req = req.body(body.map(|b| Body::from(b.to_string())).unwrap_or_else(Body::empty)).unwrap();
    let res = app.clone().oneshot(req).await.unwrap();
    let status = res.status();
    let cookie = res.headers().get_all("set-cookie").iter().filter_map(|v| v.to_str().ok()).find_map(|v| v.strip_prefix("galileo_session=")).map(|v| v.split(';').next().unwrap_or("").to_string()).filter(|v| !v.is_empty());
    let bytes = axum::body::to_bytes(res.into_body(), 1 << 20).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null), cookie)
}

fn code(v: &Value) -> &str {
    v["error"]["code"].as_str().unwrap_or("")
}

#[tokio::test]
async fn accounts_end_to_end() {
    let Some((app, pg)) = app().await else { eprintln!("GALILEO_TEST_PG not set: skipped"); return };

    // --- the first account is the Master; registration closes after it ----------------------
    let (s, setup, _) = call(&app, "GET", "/api/auth/setup", None, None).await;
    assert_eq!((s, setup["registration_open"].as_bool()), (StatusCode::OK, Some(true)));
    let (s, v, master) = call(&app, "POST", "/api/auth/register", None, Some(json!({ "email": "boss@x.io", "password": "correct horse battery", "org_name": "Clinic Co" }))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let master = master.expect("session");
    assert_eq!(v["user"]["is_master"], true);
    let org = v["org"]["id"].as_str().unwrap().to_string();
    let project = v["project"]["id"].as_str().unwrap().to_string();
    let (s, v, _) = call(&app, "POST", "/api/auth/register", None, Some(json!({ "email": "intruder@x.io", "password": "another long password" }))).await;
    assert_eq!((s, code(&v)), (StatusCode::FORBIDDEN, "registration_closed"));

    // --- the Master creates a viewer with a temporary password -------------------------------
    let (s, v, _) = call(&app, "POST", "/api/admin/users", Some(&master), Some(json!({ "email": "Ana@X.io", "name": "Ana", "org_id": org, "role": "viewer" }))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let ana_id = v["user"]["id"].as_str().unwrap().to_string();
    let temp = v["temporary_password"].as_str().unwrap().to_string();
    assert_eq!(v["user"]["must_change_password"], true);
    let (s, _, _) = call(&app, "POST", "/api/admin/users", Some(&master), Some(json!({ "email": "ana@x.io", "org_id": org, "role": "viewer" }))).await;
    assert_eq!(s, StatusCode::CONFLICT, "e-mails are unique, case-insensitively");

    // --- first sign-in: nothing but choosing a password ---------------------------------------
    let (s, v, ana) = call(&app, "POST", "/api/auth/login", None, Some(json!({ "email": "ana@x.io", "password": temp }))).await;
    assert_eq!((s, v["must_change_password"].as_bool()), (StatusCode::OK, Some(true)), "{v}");
    let ana = ana.unwrap();
    let (s, v, _) = call(&app, "GET", "/api/auth/me", Some(&ana), None).await;
    assert_eq!((s, v["user"]["must_change_password"].as_bool()), (StatusCode::OK, Some(true)));
    let boards = format!("/api/projects/{project}/boards");
    let (s, v, _) = call(&app, "GET", &boards, Some(&ana), None).await;
    assert_eq!((s, code(&v)), (StatusCode::FORBIDDEN, "password_change_required"));
    let (s, _, _) = call(&app, "POST", "/api/auth/password", Some(&ana), Some(json!({ "new_password": "short" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "the password rule applies");
    let (s, v, _) = call(&app, "POST", "/api/auth/password", Some(&ana), Some(json!({ "new_password": "a much better passphrase" }))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let (s, _, _) = call(&app, "GET", &boards, Some(&ana), None).await;
    assert_eq!(s, StatusCode::OK, "reading works once the password is changed");

    // --- a viewer reads but does not change; an override grants one permission ---------------
    let board = json!({ "name": "Mine", "panels": [] });
    let (s, _, _) = call(&app, "POST", &boards, Some(&ana), Some(board.clone())).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _, _) = call(&app, "GET", &format!("/api/projects/{project}/export"), Some(&ana), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN, "config export needs audit_export");
    let (s, v, _) = call(&app, "POST", &format!("/api/projects/{project}/query"), Some(&ana), Some(json!({ "dataset": "spans", "time_range": { "last_seconds": 3600 }, "calculations": [{ "op": "COUNT" }], "filters": [], "breakdowns": ["user.email"], "orders": [] }))).await;
    assert_eq!((s, code(&v)), (StatusCode::FORBIDDEN, "sensitive_field"));
    let (s, v, _) = call(&app, "PUT", &format!("/api/admin/users/{ana_id}/permissions"), Some(&master), Some(json!({ "org_id": org, "overrides": { "edit_content": true } }))).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    assert_eq!(v["effective"], json!(["edit_content"]));
    let (s, _, _) = call(&app, "POST", &boards, Some(&ana), Some(board)).await;
    assert_eq!(s, StatusCode::OK, "the override applies at once");
    let (_, v, _) = call(&app, "GET", &format!("/api/projects/{project}"), Some(&ana), None).await;
    assert_eq!(v["permissions"], json!(["edit_content"]));

    // --- only the Master manages accounts ------------------------------------------------------
    for (m, p, b) in [
        ("GET", "/api/admin/users".to_string(), None),
        ("POST", "/api/orgs".to_string(), Some(json!({ "name": "Mine" }))),
        ("POST", format!("/api/orgs/{org}/invites"), Some(json!({ "email": "z@x.io" }))),
        ("POST", format!("/api/admin/users/{ana_id}/reset-password"), None),
    ] {
        let (s, _, _) = call(&app, m, &p, Some(&ana), b).await;
        assert_eq!(s, StatusCode::FORBIDDEN, "{m} {p}");
    }

    // --- reset: signed out everywhere, a new temporary password --------------------------------
    let (s, v, _) = call(&app, "POST", &format!("/api/admin/users/{ana_id}/reset-password"), Some(&master), None).await;
    assert_eq!(s, StatusCode::OK, "{v}");
    let temp2 = v["temporary_password"].as_str().unwrap().to_string();
    let (s, _, _) = call(&app, "GET", "/api/auth/me", Some(&ana), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "old session is gone");
    let (s, _, _) = call(&app, "POST", "/api/auth/login", None, Some(json!({ "email": "ana@x.io", "password": "a much better passphrase" }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "old password is gone");
    let (s, v, _) = call(&app, "POST", "/api/auth/login", None, Some(json!({ "email": "ana@x.io", "password": temp2 }))).await;
    assert_eq!(v["must_change_password"], true, "{s} {v}");

    // --- an unused temporary password expires ---------------------------------------------------
    sqlx::query("UPDATE users SET temp_password_expires_at = now() - interval '1 minute' WHERE id = $1::uuid").bind(&ana_id).execute(&pg).await.unwrap();
    let (s, v, _) = call(&app, "POST", "/api/auth/login", None, Some(json!({ "email": "ana@x.io", "password": temp2 }))).await;
    assert_eq!((s, code(&v)), (StatusCode::UNAUTHORIZED, "temporary_password_expired"));

    // --- disabled accounts cannot sign in --------------------------------------------------------
    let (s, v, _) = call(&app, "POST", &format!("/api/admin/users/{ana_id}/reset-password"), Some(&master), None).await;
    assert_eq!(s, StatusCode::OK);
    let temp3 = v["temporary_password"].as_str().unwrap().to_string();
    let (s, _, _) = call(&app, "PATCH", &format!("/api/admin/users/{ana_id}"), Some(&master), Some(json!({ "disabled": true }))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, v, _) = call(&app, "POST", "/api/auth/login", None, Some(json!({ "email": "ana@x.io", "password": temp3 }))).await;
    assert_eq!((s, code(&v)), (StatusCode::FORBIDDEN, "disabled"));
    let (s, _, _) = call(&app, "PATCH", &format!("/api/admin/users/{ana_id}"), Some(&master), Some(json!({ "disabled": false }))).await;
    assert_eq!(s, StatusCode::OK);

    // --- five wrong passwords lock the account, even for the right one -------------------------
    for _ in 0..5 {
        let (s, _, _) = call(&app, "POST", "/api/auth/login", None, Some(json!({ "email": "ana@x.io", "password": "wrong password!" }))).await;
        assert_eq!(s, StatusCode::UNAUTHORIZED);
    }
    let (s, v, _) = call(&app, "POST", "/api/auth/login", None, Some(json!({ "email": "ana@x.io", "password": temp3 }))).await;
    assert_eq!((s, code(&v)), (StatusCode::TOO_MANY_REQUESTS, "locked"));
    let (s, _, _) = call(&app, "PATCH", &format!("/api/admin/users/{ana_id}"), Some(&master), Some(json!({ "unlock": true }))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _, _) = call(&app, "POST", "/api/auth/login", None, Some(json!({ "email": "ana@x.io", "password": temp3 }))).await;
    assert_eq!(s, StatusCode::OK, "the Master unlocks");
    let (s, _, _) = call(&app, "POST", "/api/auth/login", None, Some(json!({ "email": "nobody@x.io", "password": "whatever it is" }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "unknown e-mails answer like wrong passwords");

    // --- the Master cannot lock the instance out --------------------------------------------------
    let me: String = sqlx::query_scalar("SELECT id::text FROM users WHERE email = 'boss@x.io'").fetch_one(&pg).await.unwrap();
    for body in [json!({ "disabled": true }), json!({ "is_master": false })] {
        let (s, _, _) = call(&app, "PATCH", &format!("/api/admin/users/{me}"), Some(&master), Some(body)).await;
        assert_eq!(s, StatusCode::CONFLICT);
    }
    let (s, _, _) = call(&app, "DELETE", &format!("/api/admin/users/{me}"), Some(&master), None).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _, _) = call(&app, "PUT", &format!("/api/admin/users/{me}/orgs/{org}"), Some(&master), Some(json!({ "role": "viewer" }))).await;
    assert_eq!(s, StatusCode::CONFLICT, "the org keeps an owner");

    // --- changing your own password needs the current one, and keeps only this session -----------
    let (_, _, second) = call(&app, "POST", "/api/auth/login", None, Some(json!({ "email": "boss@x.io", "password": "correct horse battery" }))).await;
    let second = second.unwrap();
    let (s, v, _) = call(&app, "POST", "/api/auth/password", Some(&master), Some(json!({ "current_password": "nope", "new_password": "a brand new phrase" }))).await;
    assert_eq!((s, code(&v)), (StatusCode::BAD_REQUEST, "wrong_password"));
    let (s, v, _) = call(&app, "POST", "/api/auth/password", Some(&master), Some(json!({ "current_password": "correct horse battery", "new_password": "a brand new phrase" }))).await;
    assert_eq!((s, v["signed_out_sessions"].as_u64()), (StatusCode::OK, Some(1)));
    let (s, _, _) = call(&app, "GET", "/api/auth/me", Some(&second), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, v, _) = call(&app, "GET", "/api/auth/sessions", Some(&master), None).await;
    assert_eq!((s, v["sessions"].as_array().map(|a| a.len())), (StatusCode::OK, Some(1)));

    // --- every account action is in the audit log --------------------------------------------------
    let actions: Vec<String> = sqlx::query_scalar("SELECT DISTINCT action FROM audit_log").fetch_all(&pg).await.unwrap();
    for a in ["user.create", "user.password_change", "user.permissions", "user.password_reset", "user.disable", "user.enable", "user.unlock"] {
        assert!(actions.iter().any(|x| x == a), "audit has {a}: {actions:?}");
    }
}
