//! One ordering for record ids, so `TSK-999` lists before `TSK-1000`.
//!
//! Ids are zero-padded to three digits (four for ADRs), so a plain string
//! comparison orders them only up to 999. Past that, `TSK-1000` sorts before
//! `TSK-999`. [`compare_ids`] reads each run of digits as a number, so an id
//! orders by its prefix, then its number, then any `-NNN` group of a
//! historical `TSK-NNN-NNN` id. It also orders text that carries an id, such
//! as a path (`tasks/TSK-1000.md`), a branch name or a message that starts
//! with a path. The lists a reader sees or a command prints use it, not
//! `str::cmp`. Internal inventories keep byte order where no reader sees
//! the order: set membership, hash input, ref and path scans, and registry
//! files. A scan may hand its order to a reader, so check what consumes it.

use std::cmp::Ordering;
use std::path::Path;

/// Compare two strings with every run of ASCII digits read as a number.
///
/// Runs of any length compare without overflow. Two texts that read equal
/// (`TSK-01` and `TSK-001`) fall back to byte order, so the result is a total
/// order that agrees with `Eq` and groups equal strings next to each other.
#[must_use]
pub fn compare_ids(left: &str, right: &str) -> Ordering {
    compare_bytes(left.as_bytes(), right.as_bytes()).then_with(|| left.cmp(right))
}

/// The digit-aware comparison on bytes, equal when the texts read equal.
fn compare_bytes(a: &[u8], b: &[u8]) -> Ordering {
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (end_a, end_b) = (digit_end(a, i), digit_end(b, j));
            let (run_a, run_b) = (trim_zeros(&a[i..end_a]), trim_zeros(&b[j..end_b]));
            match run_a.len().cmp(&run_b.len()).then_with(|| run_a.cmp(run_b)) {
                Ordering::Equal => (i, j) = (end_a, end_b),
                other => return other,
            }
        } else {
            match a[i].cmp(&b[j]) {
                Ordering::Equal => (i, j) = (i + 1, j + 1),
                other => return other,
            }
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

/// [`compare_ids`] on raw bytes, for names that are not text (a git branch
/// or worktree name).
#[must_use]
pub fn compare_id_bytes(left: &[u8], right: &[u8]) -> Ordering {
    compare_bytes(left, right).then_with(|| left.cmp(right))
}

/// Compare two paths component by component with the same digit-aware
/// order, so `tasks/TSK-999.md` lists before `tasks/TSK-1000.md`.
///
/// Components compare as bytes, never as decoded text, so a name that is not
/// valid UTF-8 keeps its place; paths that read equal compare by bytes.
#[must_use]
pub fn compare_id_paths(left: &Path, right: &Path) -> Ordering {
    let mut a = left.components();
    let mut b = right.components();
    loop {
        match (a.next(), b.next()) {
            (Some(x), Some(y)) => {
                match compare_bytes(
                    x.as_os_str().as_encoded_bytes(),
                    y.as_os_str().as_encoded_bytes(),
                ) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            (Some(_), None) => return Ordering::Greater,
            (None, Some(_)) => return Ordering::Less,
            (None, None) => return left.cmp(right),
        }
    }
}

/// The entries of a map keyed by id text, in [`compare_ids`] order. A
/// `BTreeMap<String, _>` iterates by text, which misorders ids past 999.
#[must_use]
pub fn sorted_by_id<V>(map: &std::collections::BTreeMap<String, V>) -> Vec<(&String, &V)> {
    let mut entries: Vec<_> = map.iter().collect();
    entries.sort_by(|left, right| compare_ids(left.0, right.0));
    entries
}

fn digit_end(bytes: &[u8], start: usize) -> usize {
    start
        + bytes[start..]
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
}

fn trim_zeros(digits: &[u8]) -> &[u8] {
    let zeros = digits.iter().take_while(|byte| **byte == b'0').count();
    &digits[zeros..]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sorted(ids: &[&str]) -> Vec<String> {
        let mut out: Vec<String> = ids.iter().map(|id| (*id).to_string()).collect();
        out.sort_by(|a, b| compare_ids(a, b));
        out
    }

    #[test]
    fn ids_order_by_number_past_three_digits() {
        assert_eq!(
            sorted(&["TSK-10000", "TSK-1000", "TSK-999", "TSK-100", "TSK-002"]),
            ["TSK-002", "TSK-100", "TSK-999", "TSK-1000", "TSK-10000"]
        );
        assert_eq!(
            sorted(&["SPC-1000", "ADR-0002", "SPC-999", "ADR-10000", "ADR-9999"]),
            ["ADR-0002", "ADR-9999", "ADR-10000", "SPC-999", "SPC-1000"]
        );
    }

    #[test]
    fn a_historical_group_orders_after_its_epic_number() {
        assert_eq!(
            sorted(&[
                "TSK-1000-001",
                "TSK-051-008",
                "TSK-051",
                "TSK-051-010",
                "TSK-999-999"
            ]),
            [
                "TSK-051",
                "TSK-051-008",
                "TSK-051-010",
                "TSK-999-999",
                "TSK-1000-001"
            ]
        );
    }

    #[test]
    fn text_that_carries_an_id_orders_by_the_id() {
        assert_eq!(
            sorted(&[
                "task/TSK-1000-b",
                "task/TSK-999-a",
                "project-management/tasks/TSK-10000.md: bad",
                "project-management/tasks/TSK-999.md: bad",
            ]),
            [
                "project-management/tasks/TSK-999.md: bad",
                "project-management/tasks/TSK-10000.md: bad",
                "task/TSK-999-a",
                "task/TSK-1000-b"
            ]
        );
    }

    #[test]
    fn equal_numbers_with_different_padding_stay_a_total_order() {
        assert_eq!(compare_ids("TSK-001", "TSK-001"), Ordering::Equal);
        assert_ne!(compare_ids("TSK-01", "TSK-001"), Ordering::Equal);
        assert_eq!(
            compare_ids("TSK-01", "TSK-001"),
            compare_ids("TSK-001", "TSK-01").reverse()
        );
        // A run longer than any integer type still compares.
        assert_eq!(
            compare_ids(
                "TSK-99999999999999999999999",
                "TSK-100000000000000000000000"
            ),
            Ordering::Less
        );
    }

    #[test]
    fn a_map_keyed_by_id_lists_by_number() {
        let map: std::collections::BTreeMap<String, u8> = ["EPC-1000", "EPC-999", "EPC-10000"]
            .iter()
            .map(|id| ((*id).to_string(), 0))
            .collect();
        let keys: Vec<&str> = sorted_by_id(&map)
            .iter()
            .map(|(id, _)| id.as_str())
            .collect();
        assert_eq!(keys, ["EPC-999", "EPC-1000", "EPC-10000"]);
    }

    #[test]
    fn paths_order_by_component() {
        let mut paths: Vec<PathBuf> = [
            "pm/tasks/TSK-10000.md",
            "pm/tasks/TSK-1000.md",
            "pm/tasks/TSK-999.md",
            "pm/epics/EPC-010/tasks/TSK-1000-001.md",
        ]
        .iter()
        .map(PathBuf::from)
        .collect();
        paths.sort_by(|a, b| compare_id_paths(a, b));
        let shown: Vec<_> = paths.iter().map(|path| path.to_str().unwrap()).collect();
        assert_eq!(
            shown,
            [
                "pm/epics/EPC-010/tasks/TSK-1000-001.md",
                "pm/tasks/TSK-999.md",
                "pm/tasks/TSK-1000.md",
                "pm/tasks/TSK-10000.md"
            ]
        );
    }
}
