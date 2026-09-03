export type Dataset = "spans" | "logs" | "metrics";

export type CalcOp =
  | "COUNT" | "COUNT_DISTINCT" | "SUM" | "AVG" | "MIN" | "MAX"
  | "P50" | "P75" | "P90" | "P95" | "P99" | "P999" | "HEATMAP" | "RATE_PER_SEC";

export const CALC_OPS: CalcOp[] = ["COUNT", "COUNT_DISTINCT", "SUM", "AVG", "MIN", "MAX", "P50", "P75", "P90", "P95", "P99", "P999", "HEATMAP", "RATE_PER_SEC"];
export const NEEDS_FIELD = (op: CalcOp) => op !== "COUNT" && op !== "RATE_PER_SEC";

export type FilterOp = "eq" | "ne" | "gt" | "gte" | "lt" | "lte" | "contains" | "not_contains" | "starts_with" | "exists" | "not_exists" | "in" | "not_in";
export const FILTER_OPS: { v: FilterOp; label: string; needsValue: boolean }[] = [
  { v: "eq", label: "=", needsValue: true },
  { v: "ne", label: "!=", needsValue: true },
  { v: "gt", label: ">", needsValue: true },
  { v: "gte", label: ">=", needsValue: true },
  { v: "lt", label: "<", needsValue: true },
  { v: "lte", label: "<=", needsValue: true },
  { v: "contains", label: "contains", needsValue: true },
  { v: "not_contains", label: "does not contain", needsValue: true },
  { v: "starts_with", label: "starts with", needsValue: true },
  { v: "exists", label: "exists", needsValue: false },
  { v: "not_exists", label: "does not exist", needsValue: false },
  { v: "in", label: "in", needsValue: true },
  { v: "not_in", label: "not in", needsValue: true },
];

/** The API accepts symbols ("=", ">") as aliases; the UI keys everything by the canonical name. */
export function filterOpMeta(op: string) {
  return FILTER_OPS.find((o) => o.v === op) ?? FILTER_OPS.find((o) => o.label === op) ?? FILTER_OPS[0];
}

export interface Calculation { op: CalcOp; field?: string }
export interface Filter { field: string; op: FilterOp; value?: unknown }
export interface Order { field: string; direction: "asc" | "desc" }
export interface Derived { name: string; expr: string }
export interface Having { target: string; op: FilterOp; value: number }
export type TimeRange = { last_seconds: number } | { start: string; end: string };

export interface Query {
  dataset: Dataset;
  time_range: TimeRange;
  calculations: Calculation[];
  filters: Filter[];
  filter_combination?: "AND" | "OR";
  breakdowns: string[];
  orders: Order[];
  limit?: number;
  granularity?: number;
  search?: string;
  columns?: string[];
  derived?: Derived[];
  having?: Having[];
  compare_to?: string | null;
}

export const defaultQuery = (dataset: Dataset = "spans"): Query => ({
  dataset,
  time_range: { last_seconds: 3600 },
  calculations: [{ op: "COUNT" }],
  filters: [],
  filter_combination: "AND",
  breakdowns: [],
  orders: [],
  limit: 50,
});

export interface Point { ts: number; values: (number | null)[] }
export interface Group { key: string[]; totals: (number | null)[]; series: Point[]; compare_totals?: (number | null)[] | null; compare_series?: Point[] }
export interface Heatmap { field: string; log_scale: boolean; bin_edges: number[]; buckets: number[]; counts: number[][]; max_count: number; total: number }
export interface QueryResponse {
  mode: "series" | "raw" | "heatmap";
  start: string; end: string; granularity: number;
  breakdowns: string[]; calculations: string[];
  groups: Group[];
  raw?: { columns: string[]; rows: unknown[][] };
  heatmap?: Heatmap;
  stats: { elapsed: number; rows_read: number; bytes_read: number };
  sql: string[];
  compare_start?: string | null;
  compare_end?: string | null;
}
export interface ServiceNode { id: string; spans: number; errors: number; p95_ms: number; entries: number }
export interface ServiceEdge { src: string; dst: string; calls: number; errors: number; p95_ms: number; kind?: string }
export interface ServiceMapRes { nodes: ServiceNode[]; edges: ServiceEdge[]; leaves: ServiceEdge[]; start: string; end: string }
export interface ShareDetail { slug: string; title: string; kind: string; query: Query; frozen_start: string | null; frozen_end: string | null; created_at: string }

export interface FieldInfo { name: string; type: "string" | "number" | "timestamp" | "bool"; column: boolean; count: number | null }

export interface SpanNode {
  project_id: string; trace_id: string; span_id: string; parent_span_id?: string;
  name: string; kind: string; start_time: string; end_time: string;
  status: { code: "unset" | "ok" | "error"; message?: string };
  service_name: string; scope_name: string; scope_version: string;
  resource: Record<string, unknown>; attributes: Record<string, unknown>;
  events: { name: string; timestamp: string; attributes: Record<string, unknown> }[];
  links: { trace_id: string; span_id: string; attributes: Record<string, unknown> }[];
  duration_ms: number; depth: number; children: string[]; offset_ms: number; orphan: boolean;
}
export interface RepeatedQuery { statement: string; table: string; function: string; namespace: string; count: number; total_ms: number; span_ids: string[] }
export interface TraceView {
  trace_id: string; start: string; end: string; duration_ms: number; span_count: number; error_count: number;
  services: string[]; roots: string[]; root_name: string; llm_calls: number; llm_cost_usd: number;
  db_calls: number; db_ms: number; repeated_queries: RepeatedQuery[]; spans: SpanNode[];
}
export interface Caller { function: string; namespace: string; file: string; line: number; calls: number; traces: number; p50_ms: number; p95_ms: number; total_ms: number }
export interface NPlusOne { statement: string; table: string; function: string; namespace: string; file: string; traces: number; avg_repeats: number; max_repeats: number; avg_ms_per_trace: number; sample_trace: string; routes: number; sample_route: string }

export interface BubbleUpKey {
  key: string; score: number; inside_total: number; outside_total: number;
  values: { value: string; inside: number; outside: number; inside_pct: number; outside_pct: number }[];
}
export interface BubbleUpResponse { inside_count: number; outside_count: number; keys: BubbleUpKey[]; sql: string }

export interface Project { id: string; org_id: string; name: string; slug: string; created_at: string }
export interface Org { id: string; name: string; slug: string; role: string }
export interface User { id: string; email: string; name: string }
export interface Me { user: User; orgs: Org[]; projects: Project[] }

export interface ApiKey { id: string; name: string; key_prefix: string; scopes: string[]; created_at: string; last_used_at: string | null; revoked_at: string | null }
export interface SavedQuery { id: string; name: string; description: string; query: Query; created_at: string; updated_at: string }
export interface BoardVariable { name: string; field: string; label?: string; default?: string; dataset?: Dataset }
export interface Board { id: string; name: string; description: string; panels: Panel[]; created_at: string; updated_at: string; variables?: BoardVariable[]; time_range?: TimeRange | null; compare?: boolean; template?: string }
export type PanelViz = "line" | "table" | "stat" | "heatmap" | "markdown" | "issues" | "slo" | "service_map";
export interface Panel { id: string; title: string; query: Query | { markdown?: string; slo_id?: string }; viz?: PanelViz; x?: number; y?: number; w?: number; h?: number }
export interface Annotation { id: string; kind: string; board_id: string | null; panel_id: string; query_text: string; at: string | null; text: string; mentions: string[]; author: string | null; created_at: string }
export interface QueryHistoryRow { id: string; query: Query; text: string; ran_at: string }
export interface Trigger {
  id: string; name: string; description: string; query: Query; op: string; threshold: number; frequency_secs: number; window_secs: number;
  enabled: boolean; recipients: RecipientRef[]; state: "ok" | "triggered" | "error" | "muted";
  last_value: number | null; last_evaluated_at: string | null; last_triggered_at: string | null;
  warn_threshold: number | null; for_secs: number; mute_until: string | null; per_group: boolean; mode: "threshold" | "baseline" | "anomaly" | "outlier";
  baseline_factor: number; baseline_min_delta: number; breaching_since: string | null; severity: "ok" | "warn" | "critical";
  sensitivity?: number; min_value?: number; composite?: { all_of?: string[]; any_of?: string[]; within_secs?: number } | null; mutes?: { group_key: string; until: string | null }[]; last_baseline?: number | null; last_band?: number | null;
}
export interface Incident { id: string; trigger_id: string; group_key: string; severity: string; fired_at: string; acknowledged_at: string | null; acknowledged_by: string; resolved_at: string | null; peak_value: number | null; last_value: number | null; notified: number; escalated: number; notes: string; trigger_name?: string }
export interface MaintenanceWindow { id: string; name: string; starts_at: string; ends_at: string; trigger_ids: string[] }
export interface OncallSchedule { id: string; name: string; members: string[]; rotation_days: number; starts_on: string; escalation: { after_secs: number; channel_id: string }[]; now?: { user_id: string; email: string } | null }
export interface Slo {
  id: string; name: string; description: string; dataset: Dataset; total_filters: Filter[]; good_filters: Filter[];
  target_pct: number; window_days: number; burn_alerts: BurnAlert[]; state: string; last_result: SloResult | null; last_evaluated_at: string | null;
}
export interface BurnAlert { name: string; long_window_mins: number; short_window_mins: number; burn_rate: number; recipients: { type: string; url: string }[] }
export interface SloResult {
  sli: number | null; sli_pct: number | null; total: number; good: number; bad: number; allowed_bad: number;
  budget_remaining_pct: number | null; state: string;
  burn_alerts: { name: string; long_burn: number | null; short_burn: number | null; triggered: boolean; burn_rate: number }[];
  series: { ts: number; sli: number | null; total: number; good: number }[];
}
export interface Provider { id: string; name: string; kind: "anthropic" | "openai" | "ollama" | "openai_compatible"; base_url: string; has_key: boolean; headers: Record<string, string> }
export interface Route {
  id: string; alias: string; description: string; targets: { provider_id: string; model: string; price_input?: number; price_output?: number }[];
  budget: { daily_usd?: number; daily_tokens?: number; monthly_usd?: number; record_content?: boolean; cache_ttl_secs?: number; alert_recipients?: RecipientRef[]; experiment?: Experiment | null; guardrails?: Guardrails | null; semantic_cache?: { threshold: number; ttl_secs: number; embed_route?: string | null } | null; smart_routing?: boolean | null };
  rate_limit: { requests_per_minute?: number }; enabled: boolean;
}
export type GuardAction = "off" | "tag" | "redact" | "block";
export interface Guardrails { pii: GuardAction; injection: GuardAction; denylist: string[]; denylist_action?: GuardAction; max_tokens_per_user_day?: number | null; max_cost_per_user_day?: number | null }
export interface AgentRun { conversation_id: string; user_id: string; turns: number; tool_turns: number; input_tokens: number; output_tokens: number; cost_usd: number; errors: number; first_seen: string; last_seen: string; model: string; route: string; model_ms: number; traces: number }
export interface AgentTurn { timestamp: string; trace_id: string; span_id: string; duration_ms: number; status_code: string; user_id: string; model: string; input_tokens: number; output_tokens: number; cost_usd: number; route: string; tool_calls: unknown[] | null; prompt: string; completion: string; finish_reason: string; cache_hit: string; guardrail: string; ttft_ms: number; fallback_index: number; app_spans: { timestamp: string; span_id: string; name: string; kind: string; service: string; duration_ms: number; status_code: string; table: string; function: string; exception: string }[] }
export interface GoldenDataset { id: string; name: string; description: string; created_at: string; items: number }
export interface PromptCi { dataset_id: string | null; run_route: string; judge_route: string; min_score: number; required: boolean }
export interface Experiment { name: string; prompt_name: string; version_a: number; version_b: number; percent_b: number; sticky: boolean }
export interface QualityAgg { key: string; n: number; avg: number | null }
export interface EvalRun { id: string; judge_route: string; rubric: string; sample_size: number; filter_route: string; status: string; scored: number; avg_score: number | null; error: string; created_at: string; finished_at: string | null }
export interface EvalsRes { runs: EvalRun[]; by_model: QualityAgg[]; by_route: QualityAgg[]; by_prompt: QualityAgg[]; feedback: QualityAgg | null; latest: { span_id: string; score: number | null; reasoning: string; model: string; route: string }[] }
export type SpanQuality = Record<string, { feedback: { rating: number; comment: string; user: string }[]; evals: { score: number | null; reasoning: string }[] }>;
export interface PromptSummary { id: string; name: string; description: string; latest_version: number | null; versions: number; ci?: PromptCi | null; promoted_version?: number | null }
export interface PromptVersion { id: string; version: number; content: { system?: string; messages?: { role: string; content: string }[] }; note: string; created_at: string; ci_status?: string | null; ci_score?: number | null }
export interface RedactionRule { id: string; rule: { type: "key"; pattern: string; action: "drop" | "mask" | "hash" } | { type: "value"; regex: string; replacement: string }; description: string; created_at: string }

export interface Issue {
  id: string; fingerprint: string; title: string; exception_type: string; culprit: string; route: string; service_name: string;
  status: "open" | "resolved" | "ignored"; first_seen: string; last_seen: string; count: number; users: number; last_trace_id: string;
  last_version: string; resolved_at: string | null; resolved_version: string; notes: string; window_count?: number; sparkline?: number[];
}
export interface Deploy { id: string; service: string; version: string; at: string; note: string; url: string }

export interface OrgProject { id: string; name: string; slug: string; created_at: string; open_issues: number; requests?: number; errors?: number; p95_ms?: number; llm_calls?: number; llm_cost_usd?: number; services?: number; last_seen?: string; users?: number }
export interface Member { user_id: string; email: string; name: string; role: string; created_at: string }
export interface Invite { id: string; email: string; role: string; created_at: string; expires_at: string; accepted_at: string | null }
export interface ApiToken { id: string; name: string; token_prefix: string; created_at: string; expires_at: string | null; last_used_at: string | null; revoked_at: string | null }
export interface AuditRow { id: string; project_id: string | null; user_email: string; action: string; target_type: string; target_id: string; details: Record<string, unknown>; at: string }

export interface Channel { id: string; name: string; kind: "webhook" | "slack" | "discord" | "telegram" | "email"; config: Record<string, unknown>; enabled: boolean; created_at: string }
export type RecipientRef = { type: "channel"; id: string } | { type: "webhook" | "slack"; url: string } | { type: "oncall"; id: string };

export interface SessionRow { session_id: string; user_id: string; service_name: string; first_seen: string; last_seen: string; pages: number; fetches: number; errors: number; failed_fetches: number; p75_lcp: number; browser: string; last_path: string; first_path: string }
export interface SessionEvent { timestamp: string; trace_id: string; span_id: string; parent_span_id: string; name: string; duration_ms: number; status_code: string; user_id: string; service_name: string; attrs: Record<string, string>; backend?: { span_id: string; service_name: string; name: string; http_route: string; http_status_code: number; duration_ms: number; status_code: string; db_calls: number; exception_type: string } }
export interface SessionDetail { session_id: string; user_id: string; first_seen: string | null; last_seen: string | null; events: SessionEvent[] }
export interface VitalsRes { overall: { name: string; p75: number; n: number; sessions: number }[]; by_page: { path: string; vitals: Record<string, number>; n: number }[]; counts: { sessions: number; pages: number; errors: number; fetches: number; failed_fetches: number; users: number } }

export interface LogMatch { field: string; op: string; value: string }
export type LogProcessor =
  | { type: "json_parse"; when?: LogMatch | null; keep_body?: boolean }
  | { type: "regex_extract"; when?: LogMatch | null; pattern: string; field?: string }
  | { type: "rename"; when?: LogMatch | null; from: string; to: string }
  | { type: "drop"; when: LogMatch }
  | { type: "severity_map"; when?: LogMatch | null; rules: { pattern: string; severity: string }[] }
  | { type: "add_field"; when?: LogMatch | null; key: string; value: string };
export interface LogPipeline { enabled: boolean; processors: LogProcessor[] }
export interface LogMetricRule { name: string; match: LogMatch; value_from?: string | null; unit?: string; enabled?: boolean }
export interface UsageRes { per_day: { day: string; signal: string; rows: number }[]; totals: Record<string, { rows: number; est_bytes: number; bytes_per_row: number }>; today: { spans: number; logs: number; metrics: number }; cardinality: { k: string; distinct_values: number; rows: number }[]; quotas: { spans_per_day?: number | null; logs_per_day?: number | null; metrics_per_day?: number | null; mode?: string }; ingest: Record<string, number> }
