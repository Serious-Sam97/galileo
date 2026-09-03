//! DB query spans: `galileo::sql!("SELECT * FROM orders WHERE id = $1", "postgresql", { sqlx::query(...).fetch_one(&pool).await })`.

use once_cell::sync::Lazy;
use regex::Regex;

static TABLE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"(?i)\b(?:from|into|update|join)\s+[`"\[]?([A-Za-z0-9_.]+)[`"\]]?"#).unwrap());

pub fn operation(sql: &str) -> String { sql.split_whitespace().next().unwrap_or("QUERY").to_uppercase() }
pub fn table(sql: &str) -> String { TABLE.captures(sql).map(|c| c[1].to_string()).unwrap_or_default() }

/// A CLIENT span around a query future with db.* attributes and the calling function's `code.*`
/// (from the macro call site).
#[macro_export]
macro_rules! sql {
    ($statement:expr, $system:expr, $body:expr) => {{
        let __stmt: &str = $statement;
        let __op = $crate::sql::operation(__stmt);
        let __table = $crate::sql::table(__stmt);
        let __name = if __table.is_empty() { __op.clone() } else { format!("{} {}", __op, __table) };
        let __span = $crate::tracing::info_span!("db.query", otel.name = %__name, otel.kind = "client", db.system = $system, db.statement = %__stmt, db.operation = %__op, db.sql.table = %__table,
            code.function.name = %{ fn __galileo_here() {} $crate::sql::fn_name(std::any::type_name_of_val(&__galileo_here)) }, code.file.path = file!(), code.line.number = line!(), code.namespace = module_path!(),
            exception.r#type = $crate::tracing::field::Empty, exception.message = $crate::tracing::field::Empty, otel.status_code = $crate::tracing::field::Empty);
        $crate::tracing::Instrument::instrument($body, __span).await
    }};
}

/// Name of the function enclosing a `sql!` call, from the type name of a nested fn item
/// (`crate::module::handler::{{closure}}::__galileo_here` → `handler`).
pub fn fn_name(type_name: &str) -> String {
    let parts: Vec<&str> = type_name.split("::").filter(|p| !p.is_empty() && !p.starts_with("{{") && *p != "__galileo_here").collect();
    parts.last().map(|s| s.to_string()).unwrap_or_else(|| "fn".into())
}

#[cfg(test)]
mod tests {
    #[test]
    fn fn_name_from_type_path() {
        assert_eq!(super::fn_name("axum_app::order::{{closure}}::__galileo_here"), "order");
        assert_eq!(super::fn_name("odeon::media::MediaService::list::__galileo_here"), "list");
        assert_eq!(super::table("select * from \"orders\" o join items i"), "orders");
        assert_eq!(super::operation("  update x set y = 1"), "UPDATE");
    }
}
