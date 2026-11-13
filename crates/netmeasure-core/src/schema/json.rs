//! Writing JSON documents (design §5): pretty-printed in the field order of the types,
//! with every array of plain numbers compact on one line. A 100,000-sample column then
//! costs one line instead of 100,000, and the header stays readable at the top.

use serde::Serialize;

/// The document as text, ending with a newline.
pub fn to_text<T: Serialize>(value: &T) -> serde_json::Result<String> {
    let pretty = serde_json::to_string_pretty(value)?;
    let mut out = String::with_capacity(pretty.len() / 2);
    let lines: Vec<&str> = pretty.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.ends_with('[')
            && let Some(end) = number_block_end(&lines, i + 1)
        {
            let items: Vec<&str> = lines[i + 1..end]
                .iter()
                .map(|l| l.trim().trim_end_matches(','))
                .collect();
            out.push_str(line);
            out.push_str(&items.join(","));
            out.push_str(lines[end].trim_start());
            out.push('\n');
            i = end + 1;
            continue;
        }
        out.push_str(line);
        out.push('\n');
        i += 1;
    }
    Ok(out)
}

/// Index of the closing `]` line if every line from `start` up to it is one number.
fn number_block_end(lines: &[&str], start: usize) -> Option<usize> {
    for (offset, line) in lines[start..].iter().enumerate() {
        let item = line.trim();
        if item.starts_with(']') {
            return (offset > 0).then_some(start + offset);
        }
        let item = item.trim_end_matches(',');
        if item.parse::<f64>().is_err() {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    #[test]
    fn number_arrays_are_compact_the_rest_pretty_in_order() {
        #[derive(Serialize)]
        struct Doc {
            z: Vec<u32>,
            a: Value,
        }
        let d = Doc {
            z: vec![1, 2, 3],
            a: json!({ "c": ["x", "y"], "d": [], "e": [-1, 2.5] }),
        };
        let text = to_text(&d).unwrap();
        assert!(text.starts_with("{\n  \"z\": [1,2,3],\n"), "{text}");
        assert!(text.contains("\"d\": []"), "{text}");
        assert!(text.contains("\"e\": [-1,2.5]"), "{text}");
        assert!(
            text.contains("\"x\",\n"),
            "strings stay one per line: {text}"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&text).unwrap(),
            serde_json::to_value(&d).unwrap()
        );
    }
}
