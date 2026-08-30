use std::collections::HashMap;

pub fn parse_env(content: &str) -> HashMap<String, String> {
    content
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            let value = value.trim().trim_matches(['"', '\'']).to_string();
            Some((key.trim().to_string(), value))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comments_and_quoted_values() {
        let values = parse_env("# ignored\nTOKEN='secret.value'\n");
        assert_eq!(values.get("TOKEN").unwrap(), "secret.value");
    }
}
