//! Exception fingerprinting: the same bug should be one issue no matter how many times it
//! happens, and different bugs with the same exception type should be different issues.
//!
//! Culprit = the deepest *application* frame of the stack trace (`file:function`), skipping
//! library and framework frames. Fingerprint = hash(type, culprit, route).

/// Parse the deepest application frame out of a Python / JS / Java-style stack trace.
pub fn culprit_from_stacktrace(stack: &str) -> Option<String> {
    let mut best: Option<String> = None;
    for line in stack.lines() {
        let t = line.trim();
        // Python: File "/app/agenda/views.py", line 12, in list
        if let Some(rest) = t.strip_prefix("File \"") {
            let (file, tail) = rest.split_once('"')?;
            if is_library_path(file) {
                continue;
            }
            let func = tail.rsplit(" in ").next().unwrap_or("").trim();
            best = Some(format!("{}:{}", short_path(file), func));
            continue;
        }
        // Node: at fn (/app/src/x.js:12:3)   Java: at com.x.Y.method(Y.java:12)
        if let Some(rest) = t.strip_prefix("at ") {
            let (func, loc) = match rest.split_once(" (") {
                Some((f, l)) => (f.trim(), l.trim_end_matches(')')),
                None => ("", rest),
            };
            let file = loc.split(':').next().unwrap_or(loc);
            if is_library_path(file) || file.is_empty() {
                continue;
            }
            // JS traces are deepest-first, so keep the first app frame we see.
            if best.is_none() {
                best = Some(format!("{}:{}", short_path(file), if func.is_empty() { "?" } else { func }));
            }
        }
    }
    best
}

fn is_library_path(p: &str) -> bool {
    p.contains("site-packages") || p.contains("dist-packages") || p.contains("/lib/python") || p.contains("node_modules")
        || p.starts_with("node:") || p.starts_with('<') || p.contains("/usr/lib/") || p.contains("/usr/local/lib/")
}

fn short_path(p: &str) -> String {
    // "/app/agenda/views.py" → "agenda/views.py" (drop the first absolute segment when it looks like a container root)
    let trimmed = p.trim_start_matches('/');
    match trimmed.split_once('/') {
        Some(("app" | "srv" | "code" | "workspace" | "home" | "var", rest)) => rest.to_string(),
        _ => trimmed.to_string(),
    }
}

/// Stable 16-hex-char fingerprint (FNV-1a 64).
pub fn fingerprint(exception_type: &str, culprit: &str, route: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in exception_type.bytes().chain([0u8]).chain(culprit.bytes()).chain([0u8]).chain(route.bytes()) {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_deepest_app_frame() {
        let st = "Traceback (most recent call last):\n  File \"/usr/local/lib/python3.10/site-packages/django/core/handlers/exception.py\", line 55, in inner\n    response = get_response(request)\n  File \"/app/config/chaos.py\", line 40, in __call__\n    raise InvoiceSyncError(...)\nconfig.chaos.InvoiceSyncError: boom";
        assert_eq!(culprit_from_stacktrace(st).as_deref(), Some("config/chaos.py:__call__"));
        let st2 = "  File \"/app/a.py\", line 1, in outer\n  File \"/app/b/c.py\", line 9, in inner\n  File \"/usr/local/lib/python3.10/json/__init__.py\", line 3, in loads";
        assert_eq!(culprit_from_stacktrace(st2).as_deref(), Some("b/c.py:inner"));
        assert!(culprit_from_stacktrace("nothing here").is_none());
    }

    #[test]
    fn node_first_app_frame() {
        let st = "TypeError: x is not a function\n    at Object.<anonymous> (/app/node_modules/express/lib/router.js:1:1)\n    at handler (/app/src/pets.js:42:7)\n    at other (/app/src/app.js:9:1)";
        assert_eq!(culprit_from_stacktrace(st).as_deref(), Some("src/pets.js:handler"));
    }

    #[test]
    fn fingerprint_is_stable_and_discriminating() {
        let a = fingerprint("ValueError", "x.py:f", "/a/");
        assert_eq!(a, fingerprint("ValueError", "x.py:f", "/a/"));
        assert_ne!(a, fingerprint("ValueError", "x.py:g", "/a/"));
        assert_ne!(a, fingerprint("ValueError", "x.py:f", "/b/"));
        assert_eq!(a.len(), 16);
    }
}
