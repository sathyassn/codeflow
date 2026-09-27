//! The weighted fuzzy quote match of SPC-014 B1 step 2.
//!
//! All lengths and offsets are UTF-16 units, as the v1 text selector counts
//! them. The rule is pinned by the spec so the page, the server and the
//! fixtures agree on one answer; this module is its only implementation.

use crate::limits;

/// A re-anchored window of the block text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FuzzyMatch {
    pub start_utf16: usize,
    pub end_utf16: usize,
    pub score: f64,
}

/// What the search looked for: the quote and its stored context. `start`
/// is the stored start offset, or `None` when the quote has no position (an
/// entity or element label), which scores the position term 0.
pub struct Quote<'a> {
    pub exact: &'a str,
    pub prefix: &'a str,
    pub suffix: &'a str,
    pub start: Option<usize>,
}

/// The outcome of a bounded search (SPC-014 B1 step 2).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Search {
    /// The best window, scoring at least the threshold.
    Match(FuzzyMatch),
    /// No window reaches the threshold, or a bound is exceeded.
    NoMatch,
    /// The search would pass its work budget; nothing was scored.
    OverBudget,
}

/// The bounded search the service runs: the whole rule, or `OverBudget`
/// before any scoring when the edit-distance cells the rule needs exceed
/// `limits::FUZZY_WORK_BUDGET`, so re-anchoring never stalls a session.
#[must_use]
pub fn search(quote: &Quote<'_>, text: &str) -> Search {
    match scan(quote, text, Some(limits::FUZZY_WORK_BUDGET)) {
        Err(OverBudget) => Search::OverBudget,
        Ok(Some(found)) if found.score >= limits::FUZZY_THRESHOLD => Search::Match(found),
        Ok(_) => Search::NoMatch,
    }
}

/// The best window scoring at least the threshold, or `None`, with no work
/// budget: the rule as the fixtures pin it (tests only).
#[cfg(test)]
#[must_use]
pub fn best_match(quote: &Quote<'_>, text: &str) -> Option<FuzzyMatch> {
    best_candidate(quote, text).filter(|found| found.score >= limits::FUZZY_THRESHOLD)
}

/// The highest scoring window whatever its score, ties to the lowest start
/// and then the lowest end; `None` when no window exists or a bound is
/// exceeded.
#[cfg(test)]
#[must_use]
pub fn best_candidate(quote: &Quote<'_>, text: &str) -> Option<FuzzyMatch> {
    scan(quote, text, None).ok().flatten()
}

struct OverBudget;

/// The rule itself. The candidate ends of each start are found by binary
/// search, and when a budget is given the cells of every start's distance
/// table are counted first, so an over-budget search costs one pass over the
/// starts and no scoring.
fn scan(
    quote: &Quote<'_>,
    text: &str,
    budget: Option<u64>,
) -> Result<Option<FuzzyMatch>, OverBudget> {
    let exact: Vec<u16> = quote.exact.encode_utf16().collect();
    let text: Vec<u16> = text.encode_utf16().collect();
    if exact.is_empty()
        || exact.len() > limits::MAX_FUZZY_QUOTE_UTF16
        || text.len() > limits::MAX_FUZZY_TEXT_UTF16
    {
        return Ok(None);
    }
    let prefix: Vec<u16> = quote.prefix.encode_utf16().collect();
    let suffix: Vec<u16> = quote.suffix.encode_utf16().collect();
    let length = exact.len();
    let shortest = (7 * length).div_ceil(10).max(1);
    let longest = 13 * length / 10;
    let ends = window_ends(&text);
    // Each start with its range of candidate ends in `ends`.
    let plan: Vec<(usize, std::ops::Range<usize>)> = window_starts(&text)
        .into_iter()
        .filter_map(|start| {
            let low = ends.partition_point(|end| *end < start + shortest);
            let high = ends.partition_point(|end| *end <= start + longest);
            (low < high).then_some((start, low..high))
        })
        .collect();
    if let Some(budget) = budget {
        // Every edit-distance table the loop below builds: the quote against
        // each start's longest window, the prefix once per start and the
        // suffix once per candidate end (round 2: context tables included).
        let cells = |left: usize, right: usize| u64::try_from(left * right).unwrap_or(u64::MAX);
        let mut total: u64 = 0;
        for (start, range) in &plan {
            total = total
                .saturating_add(cells(length, ends[range.end - 1] - start))
                .saturating_add(cells(prefix.len(), prefix.len().min(*start)));
            for &end in &ends[range.clone()] {
                total =
                    total.saturating_add(cells(suffix.len(), suffix.len().min(text.len() - end)));
            }
            if total > budget {
                return Err(OverBudget);
            }
        }
        #[cfg(test)]
        tests::PLANNED.with(|planned| planned.set(total));
    }
    let mut best: Option<FuzzyMatch> = None;
    for (start, range) in plan {
        let prefix_score = if prefix.is_empty() {
            1.0
        } else {
            similarity(&prefix, &text[start.saturating_sub(prefix.len())..start])
        };
        let position = quote.start.map_or(0.0, |stored| {
            1.0 - count(stored.abs_diff(start)) / count(text.len().max(1))
        });
        let window_end = ends[range.end - 1];
        let distances = prefix_distances(&exact, &text[start..window_end]);
        for &end in &ends[range] {
            let window = end - start;
            let quote_score = 1.0 - count(distances[window]) / count(length.max(window));
            let suffix_score = if suffix.is_empty() {
                1.0
            } else {
                similarity(&suffix, &text[end..(end + suffix.len()).min(text.len())])
            };
            let score =
                0.5 * quote_score + 0.2 * prefix_score + 0.2 * suffix_score + 0.1 * position;
            if best.is_none_or(|current| score > current.score + 1e-12) {
                best = Some(FuzzyMatch {
                    start_utf16: start,
                    end_utf16: end,
                    score,
                });
            }
        }
    }
    Ok(best)
}

/// `sim(a, b)`: 1 minus the Levenshtein distance over the longer length, 1
/// when both are empty.
#[must_use]
pub fn similarity(left: &[u16], right: &[u16]) -> f64 {
    let longer = left.len().max(right.len());
    if longer == 0 {
        return 1.0;
    }
    let distances = prefix_distances(left, right);
    1.0 - count(distances[right.len()]) / count(longer)
}

/// A UTF-16 count as a float. Counts here are bounded by the fuzzy text
/// bound (65,536), far inside `f64`'s exact integer range.
fn count(units: usize) -> f64 {
    f64::from(u32::try_from(units).unwrap_or(u32::MAX))
}

/// The Levenshtein distance from `quote` to every prefix of `text`: entry
/// `m` is the distance to `text[..m]`.
fn prefix_distances(quote: &[u16], text: &[u16]) -> Vec<usize> {
    #[cfg(test)]
    tests::COMPUTED.with(|computed| {
        computed.set(computed.get() + u64::try_from(quote.len() * text.len()).unwrap());
    });
    // Rows run over the quote, columns over the text, so the last row holds
    // the distance to each text prefix at once.
    let mut previous: Vec<usize> = (0..=text.len()).collect();
    let mut current = vec![0; text.len() + 1];
    for (row, unit) in quote.iter().enumerate() {
        current[0] = row + 1;
        for (column, other) in text.iter().enumerate() {
            let substitution = previous[column] + usize::from(unit != other);
            current[column + 1] = substitution
                .min(previous[column + 1] + 1)
                .min(current[column] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous
}

fn is_space(unit: u16) -> bool {
    char::from_u32(u32::from(unit)).is_some_and(char::is_whitespace)
}

fn is_end_mark(unit: u16) -> bool {
    matches!(unit, 0x2c | 0x2e | 0x3b | 0x3a | 0x21 | 0x3f)
}

/// Offset 0 and every offset right after a run of whitespace.
fn window_starts(text: &[u16]) -> Vec<usize> {
    let mut starts = vec![0];
    for index in 1..text.len() {
        if is_space(text[index - 1]) && !is_space(text[index]) {
            starts.push(index);
        }
    }
    starts
}

/// The end of the text and every offset right before whitespace or one of
/// `, . ; : ! ?`.
fn window_ends(text: &[u16]) -> Vec<usize> {
    let mut ends: Vec<usize> = (0..text.len())
        .filter(|index| is_space(text[*index]) || is_end_mark(text[*index]))
        .collect();
    ends.push(text.len());
    ends.dedup();
    ends
}

#[cfg(test)]
mod tests {
    use super::*;

    fn units(value: &str) -> Vec<u16> {
        value.encode_utf16().collect()
    }

    #[test]
    fn similarity_matches_the_spec_definition() {
        assert!((similarity(&[], &[]) - 1.0).abs() < 1e-12);
        assert!(
            (similarity(&units("kitten"), &units("sitting")) - (1.0 - 3.0 / 7.0)).abs() < 1e-12
        );
        assert!((similarity(&units("abc"), &units("abc")) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn the_fixture_edit_reanchors_at_the_pinned_offsets_and_score() {
        // reanchor/cases.json "text edited, fuzzy match".
        let quote = Quote {
            exact: "The paths are in",
            prefix: "",
            suffix: " Figure 1 and the stage in Figur",
            start: Some(0),
        };
        let found = best_match(
            &quote,
            "The two paths are in Figure 1 and the stage is in Figure 2.",
        )
        .expect("the edited sentence re-anchors");
        assert_eq!((found.start_utf16, found.end_utf16), (0, 20));
        assert!((found.score - 0.8625).abs() < 5e-5, "{}", found.score);
    }

    #[test]
    fn a_rewritten_sentence_scores_below_the_threshold() {
        // reanchor/cases.json "text rewritten, fuzzy below threshold".
        let quote = Quote {
            exact: "Answers stay on this machine",
            prefix: "",
            suffix: ".",
            start: Some(0),
        };
        let text = "Everything is stored privately.";
        let best = best_candidate(&quote, text).expect("a window exists");
        assert!((best.score - 0.5833).abs() < 5e-5, "{}", best.score);
        assert!(best_match(&quote, text).is_none());
    }

    #[test]
    fn bounds_send_the_note_to_the_block_step() {
        let long = "a".repeat(limits::MAX_FUZZY_QUOTE_UTF16 + 1);
        let quote = Quote {
            exact: &long,
            prefix: "",
            suffix: "",
            start: None,
        };
        assert!(best_candidate(&quote, &long).is_none());
    }

    #[test]
    fn a_search_over_its_budget_stops_before_scoring_and_quickly() {
        // The worst case at the accepted bounds: a 512-unit quote against
        // 65,536 units of text with a window start every two units.
        let quote_text = "a".repeat(limits::MAX_FUZZY_QUOTE_UTF16);
        let text = "a ".repeat(limits::MAX_FUZZY_TEXT_UTF16 / 2);
        let quote = Quote {
            exact: &quote_text,
            prefix: "",
            suffix: "",
            start: Some(0),
        };
        let started = std::time::Instant::now();
        assert_eq!(search(&quote, &text), Search::OverBudget);
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
    }

    thread_local! {
        pub(super) static PLANNED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
        pub(super) static COMPUTED: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
    }

    /// Round 2: the finder's input, a short quote with 32 units of context
    /// on each side against the longest text, computes about 330 million
    /// cells, so it must stop over budget; and for an admitted search with
    /// context, the cells charged are exactly the cells computed.
    #[test]
    fn the_budget_charges_the_context_tables_too() {
        let exact = "a".repeat(26);
        let context = "b".repeat(32);
        let quote = Quote {
            exact: &exact,
            prefix: &context,
            suffix: &context,
            start: Some(32),
        };
        let text = "a ".repeat(limits::MAX_FUZZY_TEXT_UTF16 / 2);
        assert_eq!(search(&quote, &text), Search::OverBudget);

        let admitted = "a ".repeat(1_000);
        PLANNED.with(|planned| planned.set(0));
        COMPUTED.with(|computed| computed.set(0));
        assert_ne!(search(&quote, &admitted), Search::OverBudget);
        let planned = PLANNED.with(std::cell::Cell::get);
        let computed = COMPUTED.with(std::cell::Cell::get);
        assert!(planned > 0 && planned <= limits::FUZZY_WORK_BUDGET);
        assert_eq!(planned, computed);
    }

    #[test]
    fn a_search_within_its_budget_is_the_whole_rule() {
        let quote = Quote {
            exact: "The paths are in",
            prefix: "",
            suffix: " Figure 1 and the stage in Figur",
            start: Some(0),
        };
        let text = "The two paths are in Figure 1 and the stage is in Figure 2.";
        assert_eq!(
            search(&quote, text),
            Search::Match(best_match(&quote, text).unwrap())
        );
        let rewritten = Quote {
            exact: "Answers stay on this machine",
            prefix: "",
            suffix: ".",
            start: Some(0),
        };
        assert_eq!(
            search(&rewritten, "Everything is stored privately."),
            Search::NoMatch
        );
    }

    #[test]
    fn ties_go_to_the_lowest_start() {
        let quote = Quote {
            exact: "node",
            prefix: "",
            suffix: "",
            start: None,
        };
        let found = best_match(&quote, "node one node two").expect("matches");
        assert_eq!(found.start_utf16, 0);
    }
}
