//! Who is acting: a task-local identity attached to every span started while it is set.

use std::cell::RefCell;

use opentelemetry::trace::Span as _;
use opentelemetry::{Context, KeyValue};
use opentelemetry_sdk::trace::{Span, SpanProcessor};

#[derive(Debug, Clone, Default)]
pub struct Identity { pub user_id: Option<String>, pub email: Option<String>, pub name: Option<String>, pub tenant: Option<String> }

tokio::task_local! { pub static CURRENT: RefCell<Identity>; }

/// Run `f` with an identity; every span it creates carries `user.id`/`tenant.id`.
pub async fn with<F: std::future::Future>(id: Identity, f: F) -> F::Output { CURRENT.scope(RefCell::new(id), f).await }

/// Update the identity inside a `with` scope (e.g. after auth resolved the user).
pub fn set(id: Identity) { let _ = CURRENT.try_with(|c| *c.borrow_mut() = id); }

pub fn current() -> Option<Identity> { CURRENT.try_with(|c| c.borrow().clone()).ok() }

pub fn attributes() -> Vec<KeyValue> {
    let Some(id) = current() else { return vec![] };
    let mut v = vec![];
    if let Some(u) = id.user_id { v.push(KeyValue::new("user.id", u)); }
    if let Some(e) = id.email { v.push(KeyValue::new("user.email", e)); }
    if let Some(n) = id.name { v.push(KeyValue::new("user.name", n)); }
    if let Some(t) = id.tenant { v.push(KeyValue::new("tenant.id", t)); }
    v
}

/// Headers for a Galileo gateway call from the current identity.
pub fn gateway_headers() -> Vec<(&'static str, String)> {
    let mut h = vec![];
    if let Some(id) = current() {
        if let Some(u) = id.user_id { h.push(("x-galileo-user-id", u)); }
        if let Some(t) = id.tenant { h.push(("x-galileo-tenant-id", t)); }
    }
    h
}

#[derive(Debug)]
pub struct IdentityProcessor;
impl SpanProcessor for IdentityProcessor {
    fn on_start(&self, span: &mut Span, _cx: &Context) {
        let attrs = attributes();
        if !attrs.is_empty() { span.set_attributes(attrs); }
    }
    fn on_end(&self, _span: opentelemetry_sdk::trace::SpanData) {}
    fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult { Ok(()) }
    fn shutdown_with_timeout(&self, _timeout: std::time::Duration) -> opentelemetry_sdk::error::OTelSdkResult { Ok(()) }
}
