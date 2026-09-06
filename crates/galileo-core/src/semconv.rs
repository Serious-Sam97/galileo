//! Attribute keys that get their own columns in the store ("hot columns"). Everything else
//! lives in the attribute maps and is still queryable; these are just faster and are what the
//! UI defaults to.

pub const SERVICE_NAME: &str = "service.name";
pub const SERVICE_VERSION: &str = "service.version";
pub const DEPLOYMENT_ENV: &str = "deployment.environment.name";
pub const DEPLOYMENT_ENV_LEGACY: &str = "deployment.environment";
pub const HOST_NAME: &str = "host.name";

pub const HTTP_METHOD: &str = "http.request.method";
pub const HTTP_METHOD_LEGACY: &str = "http.method";
pub const HTTP_ROUTE: &str = "http.route";
pub const HTTP_STATUS: &str = "http.response.status_code";
pub const HTTP_STATUS_LEGACY: &str = "http.status_code";
pub const URL_PATH: &str = "url.path";
pub const HTTP_TARGET_LEGACY: &str = "http.target";

pub const DB_SYSTEM: &str = "db.system";
/// semconv ≥ 1.26 spelling (Go, .NET and newer Java instrumentations emit this one).
pub const DB_SYSTEM_NAME: &str = "db.system.name";
pub const DB_STATEMENT: &str = "db.query.text";
pub const DB_STATEMENT_LEGACY: &str = "db.statement";

pub const USER_ID: &str = "user.id";
pub const ENDUSER_ID_LEGACY: &str = "enduser.id";
/// Galileo-specific: which tenant of a multi-tenant app the request belonged to.
pub const TENANT_ID: &str = "tenant.id";

pub const EXCEPTION_TYPE: &str = "exception.type";

pub const CODE_FUNCTION: &str = "code.function.name";
pub const CODE_FUNCTION_LEGACY: &str = "code.function";
pub const CODE_NAMESPACE: &str = "code.namespace";
pub const CODE_FILE: &str = "code.file.path";
pub const CODE_FILE_LEGACY: &str = "code.filepath";
pub const CODE_LINE: &str = "code.line.number";
pub const CODE_LINE_LEGACY: &str = "code.lineno";
pub const DB_OPERATION: &str = "db.operation";
pub const DB_OPERATION_NEW: &str = "db.operation.name";
pub const DB_TABLE: &str = "db.table";
pub const DB_TABLE_NEW: &str = "db.collection.name";
pub const REQUEST_ID: &str = "request.id";
pub const EXCEPTION_MESSAGE: &str = "exception.message";

// gen_ai.* follows the OpenTelemetry GenAI semantic conventions so that spans produced by the
// Galileo gateway and by third-party instrumentation look the same.
pub const GEN_AI_SYSTEM: &str = "gen_ai.system";
pub const GEN_AI_OPERATION: &str = "gen_ai.operation.name";
pub const GEN_AI_REQUEST_MODEL: &str = "gen_ai.request.model";
pub const GEN_AI_RESPONSE_MODEL: &str = "gen_ai.response.model";
pub const GEN_AI_INPUT_TOKENS: &str = "gen_ai.usage.input_tokens";
pub const GEN_AI_OUTPUT_TOKENS: &str = "gen_ai.usage.output_tokens";
pub const GEN_AI_FINISH_REASONS: &str = "gen_ai.response.finish_reasons";
pub const GEN_AI_REQUEST_TEMPERATURE: &str = "gen_ai.request.temperature";
pub const GEN_AI_REQUEST_MAX_TOKENS: &str = "gen_ai.request.max_tokens";
pub const GEN_AI_PROMPT: &str = "gen_ai.prompt";
pub const GEN_AI_COMPLETION: &str = "gen_ai.completion";
/// Galileo-specific extensions.
pub const GEN_AI_COST_USD: &str = "gen_ai.usage.cost_usd";
pub const GEN_AI_ROUTE: &str = "gen_ai.galileo.route";
pub const GEN_AI_PROVIDER: &str = "gen_ai.galileo.provider";
pub const GEN_AI_FALLBACK_INDEX: &str = "gen_ai.galileo.fallback_index";
pub const GEN_AI_PROMPT_NAME: &str = "gen_ai.galileo.prompt.name";
pub const GEN_AI_PROMPT_VERSION: &str = "gen_ai.galileo.prompt.version";
pub const GEN_AI_STREAMING: &str = "gen_ai.galileo.streaming";
pub const GEN_AI_TTFT_MS: &str = "gen_ai.galileo.time_to_first_token_ms";

/// Resolve the first present key from a list of aliases (new + legacy semconv names).
pub fn first_str<'a>(attrs: &'a crate::Attributes, keys: &[&str]) -> Option<&'a str> {
    keys.iter().find_map(|k| attrs.get(*k).and_then(|v| v.as_str()))
}

/// First present key as a string. Integers and booleans are stringified: identity attributes such as
/// `user.id` / `tenant.id` arrive as int64 from typed SDKs (Go, Java) and must still fill the columns.
pub fn first_string(attrs: &crate::Attributes, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| {
        attrs.get(*k).and_then(|v| {
            v.as_str().map(str::to_owned)
                .or_else(|| v.as_i64().map(|i| i.to_string()))

        })
    })
}

pub fn first_i64(attrs: &crate::Attributes, keys: &[&str]) -> Option<i64> {
    keys.iter().find_map(|k| {
        attrs.get(*k).and_then(|v| {
            v.as_i64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
        })
    })
}
