pub mod api_keys;
pub mod boards;
pub mod orgs;
pub mod projects;
pub mod rules;
pub mod saved_queries;
pub mod users;

pub fn slugify(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut dash = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            dash = false;
        } else if !dash && !out.is_empty() {
            out.push('-');
            dash = true;
        }
    }
    let t = out.trim_end_matches('-').to_string();
    if t.is_empty() {
        "project".into()
    } else {
        t
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn slugs() {
        assert_eq!(super::slugify("My Cool App!"), "my-cool-app");
        assert_eq!(super::slugify("  ERP  Vet "), "erp-vet");
        assert_eq!(super::slugify("!!!"), "project");
    }
}
