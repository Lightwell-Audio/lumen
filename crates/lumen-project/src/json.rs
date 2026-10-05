//! Human-friendly JSON layout: pretty-printed structure, with arrays that
//! hold only scalars kept on one line (a note is `[0, 352800, 60, 100]`,
//! not six lines).

/// Rewrites `serde_json` pretty output so scalar-only arrays sit on one line.
/// The result parses to exactly the same value.
pub fn compact_scalar_arrays(pretty: &str) -> String {
    let mut out = String::with_capacity(pretty.len());
    // For each open array: where it starts in `out`, and whether it holds a container.
    let mut open: Vec<(usize, bool)> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for c in pretty.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '[' | '{' => {
                if let Some(parent) = open.last_mut() {
                    parent.1 = true;
                }
                if c == '[' {
                    open.push((out.len(), false));
                }
                out.push(c);
            }
            ']' => {
                out.push(c);
                if let Some((start, has_container)) = open.pop()
                    && !has_container
                {
                    let inline = inline_scalars(&out[start..]);
                    out.truncate(start);
                    out.push_str(&inline);
                }
            }
            _ => out.push(c),
        }
    }
    out
}

/// `[\n  1,\n  2\n]` becomes `[1, 2]`; whitespace inside strings is kept.
fn inline_scalars(array: &str) -> String {
    let mut out = String::with_capacity(array.len());
    let mut in_string = false;
    let mut escaped = false;
    for c in array.chars() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            ',' => out.push_str(", "),
            c if c.is_whitespace() => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn check(value: &Value) -> String {
        let pretty = serde_json::to_string_pretty(value).unwrap();
        let compact = compact_scalar_arrays(&pretty);
        let reparsed: Value = serde_json::from_str(&compact).unwrap();
        assert_eq!(&reparsed, value);
        compact
    }

    #[test]
    fn scalar_arrays_go_on_one_line() {
        let text = check(&json!({ "notes": [[0, 352_800, 60, 100], [5, 6, 7, 8]] }));
        assert!(text.contains("[0, 352800, 60, 100]"), "{text}");
        assert!(text.contains("\n"), "outer structure stays pretty: {text}");
    }

    #[test]
    fn strings_with_brackets_commas_and_spaces_survive() {
        check(&json!({ "a": ["x, [y]", "  spaced  ", "quote \" and \\ slash"], "b": [] }));
    }

    #[test]
    fn nested_containers_stay_pretty() {
        let text = check(&json!([[1, [2, 3]], {"k": [4]}]));
        assert!(text.contains("[2, 3]"), "{text}");
    }
}
