//! `galileo` CLI. Config: ~/.galileo/config.toml (`url`, `token`, `project`), or GALILEO_URL /
//! GALILEO_TOKEN / GALILEO_PROJECT.

use std::io::Read;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Parser)]
#[command(name = "galileo", version, about = "Galileo command line")]
struct Cli {
    /// API url (default from config or GALILEO_URL)
    #[arg(long, global = true)]
    url: Option<String>,
    /// Project id (default from config or GALILEO_PROJECT)
    #[arg(long, global = true)]
    project: Option<String>,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Store the API url, a personal token (glt_…) and a default project
    Login { #[arg(long)] url: String, #[arg(long)] token: String, #[arg(long)] project: Option<String> },
    /// Run a query in the text DSL: galileo query "spans | p95(duration_ms) by http_route"
    Query { text: String, #[arg(long, default_value_t = 3600)] last: i64, #[arg(long)] json: bool },
    /// Tail logs (polls every 2 s): galileo tail --filter "severity = ERROR"
    Tail { #[arg(long, default_value = "")] filter: String, #[arg(long, default_value_t = 2)] every: u64 },
    /// Record a deploy marker: galileo deploy 1.4.4 --service melea-api
    Deploy { version: String, #[arg(long, default_value = "")] service: String, #[arg(long, default_value = "")] note: String },
    /// Export the project configuration (YAML) to stdout
    Export { #[arg(long, default_value = "yaml")] format: String },
    /// Import a configuration bundle (YAML/JSON) from a file or stdin
    Import { file: Option<PathBuf>, #[arg(long)] dry_run: bool },
    /// Evaluate a trigger now: galileo trigger test "request volume"
    Trigger { #[command(subcommand)] cmd: TriggerCmd },
    /// Who am I / which projects
    Whoami,
}

#[derive(Subcommand)]
enum TriggerCmd { Test { name: String }, List }

#[derive(Serialize, Deserialize, Default)]
struct Config { url: Option<String>, token: Option<String>, project: Option<String> }

fn config_path() -> PathBuf { dirs::home_dir().unwrap_or_default().join(".galileo").join("config.toml") }
fn load_config() -> Config { std::fs::read_to_string(config_path()).ok().and_then(|s| toml::from_str(&s).ok()).unwrap_or_default() }

struct Client { url: String, token: String, project: String, http: reqwest::blocking::Client }

impl Client {
    fn new(cli: &Cli) -> Result<Self> {
        let cfg = load_config();
        let url = cli.url.clone().or_else(|| std::env::var("GALILEO_URL").ok()).or(cfg.url).unwrap_or_else(|| "http://localhost:8080".into()).trim_end_matches('/').to_string();
        let token = std::env::var("GALILEO_TOKEN").ok().or(cfg.token).ok_or_else(|| anyhow!("no token: run `galileo login --url … --token glt_…` (Settings → Personal tokens)"))?;
        let project = cli.project.clone().or_else(|| std::env::var("GALILEO_PROJECT").ok()).or(cfg.project).unwrap_or_default();
        Ok(Self { url, token, project, http: reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(120)).build()? })
    }
    fn need_project(&self) -> Result<&str> { if self.project.is_empty() { Err(anyhow!("no project: pass --project <id> or set it with `galileo login`")) } else { Ok(&self.project) } }
    fn req(&self, method: reqwest::Method, path: &str) -> reqwest::blocking::RequestBuilder { self.http.request(method, format!("{}{}", self.url, path)).bearer_auth(&self.token) }
    fn json(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value> {
        let mut r = self.req(method, path);
        if let Some(b) = body { r = r.json(&b); }
        let resp = r.send().context("request failed")?;
        let status = resp.status();
        let text = resp.text()?;
        if !status.is_success() { return Err(anyhow!("{status}: {}", serde_json::from_str::<Value>(&text).ok().and_then(|v| v.pointer("/error/message").and_then(|m| m.as_str()).map(str::to_owned)).unwrap_or(text))); }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }
}

fn fmt_num(v: &Value) -> String { match v { Value::Number(n) => n.as_f64().map(|f| if f.fract() == 0.0 { format!("{f:.0}") } else if f.abs() < 10.0 { format!("{f:.3}") } else { format!("{f:.1}") }).unwrap_or_default(), Value::Null => "–".into(), o => o.to_string() } }

fn main() -> Result<()> {
    let cli = Cli::parse();
    if let Cmd::Login { url, token, project } = &cli.cmd {
        let cfg = Config { url: Some(url.trim_end_matches('/').to_string()), token: Some(token.clone()), project: project.clone() };
        std::fs::create_dir_all(config_path().parent().unwrap())?;
        std::fs::write(config_path(), toml::to_string(&cfg)?)?;
        let c = Client::new(&cli)?;
        let me = c.json(reqwest::Method::GET, "/api/auth/me", None)?;
        println!("signed in as {} · {} project(s) · config saved to {}", me.pointer("/user/email").and_then(|e| e.as_str()).unwrap_or("?"), me.get("projects").and_then(|p| p.as_array()).map(|a| a.len()).unwrap_or(0), config_path().display());
        return Ok(());
    }
    let c = Client::new(&cli)?;
    match &cli.cmd {
        Cmd::Login { .. } => unreachable!(),
        Cmd::Whoami => {
            let me = c.json(reqwest::Method::GET, "/api/auth/me", None)?;
            println!("{}", me.pointer("/user/email").and_then(|e| e.as_str()).unwrap_or("?"));
            for p in me.get("projects").and_then(|p| p.as_array()).cloned().unwrap_or_default() { println!("  {}  {}{}", p.get("id").and_then(|x| x.as_str()).unwrap_or(""), p.get("name").and_then(|x| x.as_str()).unwrap_or(""), if p.get("id").and_then(|x| x.as_str()) == Some(c.project.as_str()) { "  (default)" } else { "" }); }
        }
        Cmd::Query { text, last, json: as_json } => {
            let pid = c.need_project()?;
            let parsed = c.json(reqwest::Method::POST, &format!("/api/projects/{pid}/query/parse"), Some(json!({ "text": text })))?;
            if parsed.get("ok") != Some(&Value::Bool(true)) { return Err(anyhow!("{}", parsed.get("error").and_then(|e| e.as_str()).unwrap_or("parse error"))); }
            let mut q = parsed["query"].clone();
            q["time_range"] = json!({ "last_seconds": last });
            let res = c.json(reqwest::Method::POST, &format!("/api/projects/{pid}/query"), Some(q))?;
            if *as_json { println!("{}", serde_json::to_string_pretty(&res)?); return Ok(()); }
            if let Some(raw) = res.get("raw") {
                let cols: Vec<String> = raw["columns"].as_array().map(|a| a.iter().map(|c| c.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
                let keep: Vec<usize> = cols.iter().enumerate().filter(|(_, c)| !matches!(c.as_str(), "attrs" | "resource" | "events" | "project_id")).map(|(i, _)| i).collect();
                println!("{}", keep.iter().map(|&i| cols[i].clone()).collect::<Vec<_>>().join("\t"));
                for row in raw["rows"].as_array().cloned().unwrap_or_default() { println!("{}", keep.iter().map(|&i| row.get(i).map(|v| match v { Value::String(s) => s.clone(), o => fmt_num(o) }).unwrap_or_default()).collect::<Vec<_>>().join("\t")); }
            } else {
                let bd: Vec<String> = res["breakdowns"].as_array().map(|a| a.iter().map(|c| c.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
                let calcs: Vec<String> = res["calculations"].as_array().map(|a| a.iter().map(|c| c.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
                println!("{}", bd.iter().chain(calcs.iter()).cloned().collect::<Vec<_>>().join("\t"));
                for g in res["groups"].as_array().cloned().unwrap_or_default() { let mut cells: Vec<String> = g["key"].as_array().map(|a| a.iter().map(|k| k.as_str().unwrap_or("∅").to_string()).collect()).unwrap_or_default(); cells.extend(g["totals"].as_array().cloned().unwrap_or_default().iter().map(fmt_num)); println!("{}", cells.join("\t")); }
                if let Some(src) = res.get("sql").and_then(|s| s.as_array()) { if src.iter().any(|x| x.as_str().map(|s| s.contains("spans_red_1m")).unwrap_or(false)) { eprintln!("(answered from the long-retention rollup)"); } }
            }
        }
        Cmd::Tail { filter, every } => {
            let pid = c.need_project()?;
            let text = if filter.is_empty() { "logs".to_string() } else { format!("logs | where {filter}") };
            let parsed = c.json(reqwest::Method::POST, &format!("/api/projects/{pid}/query/parse"), Some(json!({ "text": text })))?;
            if parsed.get("ok") != Some(&Value::Bool(true)) { return Err(anyhow!("{}", parsed.get("error").and_then(|e| e.as_str()).unwrap_or("parse error"))); }
            let mut q = parsed["query"].clone(); q["limit"] = json!(100);
            let mut seen = std::collections::HashSet::new();
            let mut last_ts = String::new();
            loop {
                q["time_range"] = json!({ "last_seconds": 120 });
                let res = c.json(reqwest::Method::POST, &format!("/api/projects/{pid}/query"), Some(q.clone()))?;
                let cols: Vec<String> = res.pointer("/raw/columns").and_then(|a| a.as_array()).map(|a| a.iter().map(|c| c.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default();
                let ix = |n: &str| cols.iter().position(|c| c == n);
                let mut rows = res.pointer("/raw/rows").and_then(|a| a.as_array()).cloned().unwrap_or_default();
                rows.reverse();
                for r in rows {
                    let ts = ix("timestamp").and_then(|i| r.get(i)).and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let key = format!("{ts}|{}", ix("body").and_then(|i| r.get(i)).map(|v| v.to_string()).unwrap_or_default());
                    if ts < last_ts || !seen.insert(key) { continue; }
                    last_ts = ts.clone();
                    println!("{} {:<5} {} {}{}", &ts[..19.min(ts.len())], ix("severity").and_then(|i| r.get(i)).and_then(|v| v.as_str()).unwrap_or(""), ix("service_name").and_then(|i| r.get(i)).and_then(|v| v.as_str()).unwrap_or(""), ix("body").and_then(|i| r.get(i)).and_then(|v| v.as_str()).unwrap_or(""), ix("user_id").and_then(|i| r.get(i)).and_then(|v| v.as_str()).filter(|u| !u.is_empty()).map(|u| format!("  user={u}")).unwrap_or_default());
                }
                std::thread::sleep(std::time::Duration::from_secs(*every));
            }
        }
        Cmd::Deploy { version, service, note } => {
            let pid = c.need_project()?;
            let r = c.json(reqwest::Method::POST, &format!("/api/projects/{pid}/deploys"), Some(json!({ "version": version, "service": service, "note": note })))?;
            println!("deploy {} recorded{}", version, r.get("deploy").and_then(|d| d.get("at")).and_then(|a| a.as_str()).map(|a| format!(" at {a}")).unwrap_or_default());
        }
        Cmd::Export { format } => {
            let pid = c.need_project()?;
            let resp = c.req(reqwest::Method::GET, &format!("/api/projects/{pid}/export?format={format}")).send()?;
            if !resp.status().is_success() { return Err(anyhow!("{}", resp.status())); }
            print!("{}", resp.text()?);
        }
        Cmd::Import { file, dry_run } => {
            let pid = c.need_project()?;
            let mut body = String::new();
            match file { Some(f) => body = std::fs::read_to_string(f).with_context(|| format!("reading {}", f.display()))?, None => { std::io::stdin().read_to_string(&mut body)?; } }
            let resp = c.req(reqwest::Method::POST, &format!("/api/projects/{pid}/import?dry_run={dry_run}")).header("content-type", "application/yaml").body(body).send()?;
            let status = resp.status(); let text = resp.text()?;
            if !status.is_success() { return Err(anyhow!("{status}: {text}")); }
            let v: Value = serde_json::from_str(&text)?;
            let changes = v.get("changes").and_then(|c| c.as_array()).cloned().unwrap_or_default();
            if changes.is_empty() { println!("{}no changes", if *dry_run { "dry run: " } else { "" }); }
            for ch in changes { println!("{} {} {}: {}", if *dry_run { "would" } else { "applied" }, ch["action"].as_str().unwrap_or(""), ch["section"].as_str().unwrap_or(""), ch["name"].as_str().unwrap_or("")); }
        }
        Cmd::Trigger { cmd } => {
            let pid = c.need_project()?;
            let list = c.json(reqwest::Method::GET, &format!("/api/projects/{pid}/triggers"), None)?;
            let triggers = list["triggers"].as_array().cloned().unwrap_or_default();
            match cmd {
                TriggerCmd::List => { for t in &triggers { println!("{:<40} {:<10} {}", t["name"].as_str().unwrap_or(""), t["state"].as_str().unwrap_or(""), fmt_num(&t["last_value"])); } }
                TriggerCmd::Test { name } => {
                    let t = triggers.iter().find(|t| t["name"].as_str() == Some(name)).ok_or_else(|| anyhow!("no trigger named '{name}'"))?;
                    let r = c.json(reqwest::Method::POST, &format!("/api/projects/{pid}/triggers/{}/test", t["id"].as_str().unwrap_or("")), Some(json!({})))?;
                    let e = r.get("evaluation").cloned().unwrap_or(r);
                    println!("{}: severity {} value {}{}", name, e["severity"].as_str().unwrap_or("?"), fmt_num(&e["value"]), e.get("baseline").filter(|b| !b.is_null()).map(|b| format!(" baseline {}", fmt_num(b))).unwrap_or_default());
                }
            }
        }
    }
    Ok(())
}
