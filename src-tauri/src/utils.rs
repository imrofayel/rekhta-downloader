use regex::Regex;

pub fn sanitize_filename(name: &str) -> String {
    let re = Regex::new(r#"[<>:?*/\\|"]+"#).unwrap();
    let result = re.replace_all(name, "_");
    result.to_string().trim().to_string()
}
