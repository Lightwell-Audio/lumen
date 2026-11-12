//! Finding rows: the fuzzy match and its score, and the values a query can
//! name (a tempo, a bar).

use super::Entry;

/// Where `query` matches `text` as a subsequence, ignoring case: a score
/// (consecutive letters and starts of words count more; a text that starts
/// with the query more still, and the whole text most) and the matched
/// characters' indices. `None` when it does not match.
pub(crate) fn fuzzy(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    let query: Vec<char> = query.trim().to_lowercase().chars().collect();
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let text: Vec<char> = text.to_lowercase().chars().collect();
    let mut matched = Vec::with_capacity(query.len());
    let mut score = 0;
    let mut q = 0;
    for (i, &c) in text.iter().enumerate() {
        if q < query.len() && c == query[q] {
            let start = i == 0 || !text[i - 1].is_alphanumeric();
            let follows = matched.last() == Some(&(i.wrapping_sub(1)));
            score += 1 + if follows { 3 } else { 0 } + if start { 4 } else { 0 };
            matched.push(i);
            q += 1;
        }
    }
    if q < query.len() {
        return None;
    }
    if text.starts_with(&query) {
        score += 10;
        if text.len() == query.len() {
            score += 10;
        }
    }
    Some((score, matched))
}

/// Values in the query: a tempo and a bar for a number ("128", "128 bpm",
/// "bar 17", "tempo 140").
pub(super) fn values(query: &str) -> Vec<Entry> {
    let q = query.trim().to_lowercase();
    let number = |s: &str| s.trim().parse::<f64>().ok().filter(|n| n.is_finite());
    let tempo = |n: f64| (20.0..=999.0).contains(&n).then_some(Entry::Tempo(n));
    let bar = |n: f64| (n >= 1.0 && n.fract() == 0.0).then_some(Entry::Bar(n as i64));
    if let Some(n) = q.strip_prefix("bar").and_then(number) {
        return bar(n).into_iter().collect();
    }
    if let Some(n) = q
        .strip_prefix("tempo")
        .or_else(|| q.strip_suffix("bpm"))
        .and_then(number)
    {
        return tempo(n).into_iter().collect();
    }
    match number(&q) {
        Some(n) => tempo(n).into_iter().chain(bar(n)).collect(),
        None => Vec::new(),
    }
}

/// "128", "128.5".
pub(super) fn trim_number(n: f64) -> String {
    let text = format!("{n:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fuzzy_search_prefers_word_starts_and_runs() {
        let (dup, at) = fuzzy("dup", "Duplicate 2 clips").expect("matches");
        assert_eq!(at, [0, 1, 2]);
        let (spread, _) = fuzzy("dup", "Add audio track up").expect("matches too");
        assert!(dup > spread, "a run at a word start scores higher");
        assert!(fuzzy("xyz", "Duplicate").is_none());
        assert_eq!(fuzzy("  ", "Anything"), Some((0, Vec::new())));
        let (_, at) = fuzzy("AT", "Add audio track").expect("ignores case");
        assert_eq!(at, [0, 10]);
        // The command named exactly so beats word starts elsewhere.
        let (cut, _) = fuzzy("cut", "Cut").expect("matches");
        let (initials, _) = fuzzy("cut", "Next clip or unfold track").expect("matches");
        assert!(cut > initials);
    }

    #[test]
    fn numbers_become_a_tempo_and_a_bar() {
        assert_eq!(values("128"), [Entry::Tempo(128.0), Entry::Bar(128)]);
        assert_eq!(values("12"), [Entry::Bar(12)], "too slow for a tempo");
        assert_eq!(values("128.5"), [Entry::Tempo(128.5)], "not a bar");
        assert_eq!(values("bar 1000"), [Entry::Bar(1000)]);
        assert_eq!(values("140 bpm"), [Entry::Tempo(140.0)]);
        assert_eq!(values("tempo 90"), [Entry::Tempo(90.0)]);
        assert_eq!(values("tempo 5"), []);
        assert_eq!(values("kick"), []);
        assert_eq!(trim_number(128.0), "128");
        assert_eq!(trim_number(128.25), "128.25");
    }
}
