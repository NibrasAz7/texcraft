//! Occurrence-mirror editing (TeXstudio "cursor mirrors" parity).
//!
//! The user marks other occurrences of the selection; typing at the
//! primary caret then replicates into every marked site. egui's
//! `TextEdit` exposes a single caret, so this module is the pure
//! edit-replication engine — the app wires keystrokes in (`Ctrl+M`
//! marks, Esc clears) and paints the extra ranges itself.
//!
//! Conventions (same as `edits.rs`):
//! - All positions and ranges are **char indices**, never bytes — the
//!   caller converts for `CCursor`/byte work.
//! - Input is hostile: inside-out or out-of-range mirrors are dropped
//!   rather than edited (char indices can't slice off a UTF-8 boundary),
//!   `pos`/`count` clamp, and overlapping sites are deduped.
//! - Edits apply back-to-front so earlier sites' offsets stay valid.

/// Hard cap on [`occurrences`] matches — the source is untrusted.
const MAX_MATCHES: usize = 128;

/// A live mirror session: the primary selection's char range plus the
/// char ranges that mirror it (all same length — they hold the same
/// text).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mirrors {
    /// Char range of the selection the caret lives in.
    pub primary: (usize, usize),
    /// Other occurrences replicating the primary, sorted by start.
    pub others: Vec<(usize, usize)>,
}

/// All non-overlapping occurrences of `needle` in `src` as char ranges
/// (empty needle → empty vec; capped at [`MAX_MATCHES`]).
pub fn occurrences(src: &str, needle: &str) -> Vec<(usize, usize)> {
    if needle.is_empty() {
        return Vec::new();
    }
    let width = needle.chars().count();
    let mut out = Vec::new();
    // Char index of each match's byte offset — walked in lockstep with
    // `match_indices` (already non-overlapping, leftmost-first) so the
    // conversion stays O(chars), not O(matches · chars).
    let mut chars = src.char_indices().peekable();
    let mut ci = 0usize;
    for (byte, _) in src.match_indices(needle) {
        while let Some(&(b, _)) = chars.peek() {
            if b >= byte {
                break;
            }
            chars.next();
            ci += 1;
        }
        out.push((ci, ci + width));
        if out.len() >= MAX_MATCHES {
            break;
        }
    }
    out
}

/// Apply "insert `text` at char index `pos`" to `src` at the primary
/// range AND each mirror range — the same insertion lands at every
/// range's start + relative offset, so the sites keep holding identical
/// text. Returns the new source and the adjusted ranges (later sites
/// shift right by the cumulative inserted chars). `pos` clamps into the
/// primary range.
pub fn insert_replicated(src: &str, m: &Mirrors, pos: usize, text: &str) -> (String, Mirrors) {
    let (primary, sites) = sites(src, m);
    let rel = pos.clamp(primary.0, primary.1).saturating_sub(primary.0);
    let n = text.chars().count();
    let mut buf = src.to_string();
    // Back-to-front: byte offsets of not-yet-edited sites stay valid.
    for &(s, e) in sites.iter().rev() {
        let caret = s.saturating_add(rel.min(e - s));
        buf.insert_str(byte_of_char(&buf, caret), text);
    }
    // Site i moves right by `n` chars per site before it and grows by
    // `n` itself — the insertion lands inside (or at the end of) it.
    let mut adjusted = Vec::with_capacity(sites.len());
    for (i, &(s, e)) in sites.iter().enumerate() {
        let shift = n.saturating_mul(i);
        adjusted.push((s.saturating_add(shift), e.saturating_add(shift).saturating_add(n)));
    }
    (buf, rebuild(primary, &sites, &adjusted))
}

/// Same for "delete `count` chars ending at `pos`" (backspace): `count`
/// clamps to the chars each site actually has before its caret, so a
/// hostile short mirror deletes less instead of eating its neighbours.
pub fn delete_replicated(src: &str, m: &Mirrors, pos: usize, count: usize) -> (String, Mirrors) {
    let (primary, sites) = sites(src, m);
    let rel = pos.clamp(primary.0, primary.1).saturating_sub(primary.0);
    let takes: Vec<usize> = sites.iter().map(|&(s, e)| count.min(s.saturating_add(rel).min(e) - s)).collect();
    let mut buf = src.to_string();
    for (i, &(s, e)) in sites.iter().enumerate().rev() {
        let Some(&take) = takes.get(i) else { continue };
        if take == 0 {
            continue;
        }
        let caret = s.saturating_add(rel).min(e);
        let bs = byte_of_char(&buf, caret - take);
        let be = byte_of_char(&buf, caret);
        buf.replace_range(bs..be, "");
    }
    // Site i loses `take_i` chars inside and moves left by the total
    // deleted before it (all earlier deletions lie inside earlier
    // non-overlapping sites, so they are strictly before it).
    let mut adjusted = Vec::with_capacity(sites.len());
    let mut deleted_before = 0usize;
    for (i, &(s, e)) in sites.iter().enumerate() {
        let take = takes.get(i).copied().unwrap_or(0);
        adjusted.push((s.saturating_sub(deleted_before), e.saturating_sub(deleted_before).saturating_sub(take)));
        deleted_before = deleted_before.saturating_add(take);
    }
    (buf, rebuild(primary, &sites, &adjusted))
}

/// Edit sites for one pass: the primary range clamped into `src`, plus
/// `others` that are valid (`s <= e <= char count`), not the primary
/// itself, and non-overlapping — sorted by start. Returns `(primary,
/// sites)` where `sites` includes the primary's slot.
fn sites(src: &str, m: &Mirrors) -> ((usize, usize), Vec<(usize, usize)>) {
    let total = src.chars().count();
    let ps = m.primary.0.min(total);
    let primary = (ps, m.primary.1.min(total).max(ps));
    let mut others: Vec<(usize, usize)> =
        m.others.iter().copied().filter(|&(s, e)| s <= e && e <= total && (s, e) != primary && !(s < primary.1 && e > primary.0)).collect();
    others.sort_unstable();
    others.dedup();
    // Keep the first site on any overlap so the rest is strictly
    // non-overlapping — overlapping mirrors would double-edit the same
    // text.
    let mut keep: Vec<(usize, usize)> = Vec::with_capacity(others.len());
    for &(s, e) in &others {
        if keep.last().is_some_and(|&(_, pe)| s < pe) {
            continue;
        }
        keep.push((s, e));
    }
    keep.push(primary);
    keep.sort_unstable();
    (primary, keep)
}

/// Reassemble the session from adjusted sites: the primary's new range
/// plus every other adjusted range, still sorted by start.
fn rebuild(primary: (usize, usize), sites: &[(usize, usize)], adjusted: &[(usize, usize)]) -> Mirrors {
    let mut out = Mirrors { primary, others: Vec::with_capacity(adjusted.len()) };
    for (i, &(s, e)) in sites.iter().enumerate() {
        let Some(&r) = adjusted.get(i) else { continue };
        if (s, e) == primary {
            out.primary = r;
        } else {
            out.others.push(r);
        }
    }
    out
}

/// Byte offset of char index `ci` in `s` (`s.len()` past the end) —
/// always on a char boundary.
fn byte_of_char(s: &str, ci: usize) -> usize {
    s.char_indices().nth(ci).map(|(b, _)| b).unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Build a session from `src`'s occurrences of `needle`, taking the
    /// first as the primary.
    fn session(src: &str, needle: &str) -> Mirrors {
        let occ = occurrences(src, needle);
        Mirrors { primary: occ[0], others: occ[1..].to_vec() }
    }

    // ---------- occurrences ----------

    #[test]
    fn occurrences_char_ranges() {
        assert_eq!(occurrences("foo bar foo", "foo"), vec![(0, 3), (8, 11)]);
        assert!(occurrences("abc", "z").is_empty());
        assert!(occurrences("", "x").is_empty());
    }

    #[test]
    fn occurrences_empty_needle() {
        assert!(occurrences("abc", "").is_empty());
        assert!(occurrences("", "").is_empty());
    }

    #[test]
    fn occurrences_non_overlapping() {
        // Leftmost non-overlapping: "aa" in "aaaa" is 2 sites, not 3.
        assert_eq!(occurrences("aaaa", "aa"), vec![(0, 2), (2, 4)]);
        assert_eq!(occurrences("aaa", "aa"), vec![(0, 2)]);
    }

    #[test]
    fn occurrences_multibyte_char_indices() {
        // "αβ" is 2 chars / 4 bytes — ranges are char indices.
        assert_eq!(occurrences("αβ x αβ", "αβ"), vec![(0, 2), (5, 7)]);
        assert_eq!(occurrences("汉汉", "汉"), vec![(0, 1), (1, 2)]);
    }

    #[test]
    fn occurrences_capped() {
        let src = "a".repeat(500);
        let m = occurrences(&src, "a");
        assert_eq!(m.len(), MAX_MATCHES);
        assert_eq!(m[127], (127, 128));
    }

    // ---------- insert_replicated ----------

    #[test]
    fn insert_replicates_to_all_sites() {
        let src = "foo and foo and foo";
        let m = session(src, "foo");
        let (out, m2) = insert_replicated(src, &m, 3, "X");
        assert_eq!(out, "fooX and fooX and fooX");
        assert_eq!(m2.primary, (0, 4));
        assert_eq!(m2.others, vec![(9, 13), (18, 22)]);
    }

    #[test]
    fn insert_mid_range_multibyte() {
        let src = "αα αα";
        let m = session(src, "αα"); // (0,2), (3,5) in chars
        let (out, m2) = insert_replicated(src, &m, 1, "β");
        assert_eq!(out, "αβα αβα");
        assert_eq!(m2.primary, (0, 3));
        assert_eq!(m2.others, vec![(4, 7)]);
    }

    #[test]
    fn insert_clamps_pos_and_empty_text() {
        let src = "foo foo";
        let m = session(src, "foo");
        // Caret past the primary end clamps into the range.
        let (out, _) = insert_replicated(src, &m, 99, "!");
        assert_eq!(out, "foo! foo!");
        // Empty text: no source change, ranges unchanged.
        let (same, m2) = insert_replicated(src, &m, 1, "");
        assert_eq!(same, src);
        assert_eq!(m2.primary, (0, 3));
        assert_eq!(m2.others, vec![(4, 7)]);
    }

    #[test]
    fn insert_drops_bad_mirrors() {
        // Duplicate of the primary, out of range, inside-out → dropped.
        let m = Mirrors { primary: (0, 3), others: vec![(0, 3), (50, 60), (4, 2)] };
        let (out, m2) = insert_replicated("foo bar foo", &m, 3, "X");
        assert_eq!(out, "fooX bar foo");
        assert_eq!(m2.primary, (0, 4));
        assert!(m2.others.is_empty());
    }

    #[test]
    fn insert_keeps_sites_sorted() {
        // Hostile unsorted `others` — output ranges stay sorted.
        let m = Mirrors { primary: (8, 11), others: vec![(16, 19), (0, 3)] };
        let (out, m2) = insert_replicated("foo and foo and foo", &m, 11, "X");
        assert_eq!(out, "fooX and fooX and fooX");
        assert_eq!(m2.primary, (9, 13));
        assert_eq!(m2.others, vec![(0, 4), (18, 22)]);
    }

    #[test]
    fn insert_drops_overlapping_mirror() {
        // (1,6) overlaps the primary (0,3) → dropped, not double-edited.
        let m = Mirrors { primary: (0, 3), others: vec![(1, 6), (8, 11)] };
        let (out, m2) = insert_replicated("foo bar foo", &m, 3, "X");
        assert_eq!(out, "fooX bar fooX");
        assert_eq!(m2.others, vec![(9, 13)]);
    }

    // ---------- delete_replicated ----------

    #[test]
    fn delete_replicates_backspace() {
        let src = "foo and foo and foo";
        let m = session(src, "foo");
        let (out, m2) = delete_replicated(src, &m, 3, 1);
        assert_eq!(out, "fo and fo and fo");
        assert_eq!(m2.primary, (0, 2));
        assert_eq!(m2.others, vec![(7, 9), (14, 16)]);
    }

    #[test]
    fn delete_clamps_count_per_site() {
        let src = "foo foo";
        let m = session(src, "foo");
        // Caret at the range start: nothing before it, deletes nothing.
        let (out, _) = delete_replicated(src, &m, 0, 5);
        assert_eq!(out, src);
        // Caret mid-range (rel 2): count 5 clamps to the 2 available.
        let (out, m2) = delete_replicated(src, &m, 2, 5);
        assert_eq!(out, "o o");
        assert_eq!(m2.primary, (0, 1));
        assert_eq!(m2.others, vec![(2, 3)]);
    }

    #[test]
    fn delete_multibyte_whole_range() {
        let src = "αα αα";
        let m = session(src, "αα");
        let (out, m2) = delete_replicated(src, &m, 2, 2);
        assert_eq!(out, " ");
        // Both ranges collapse to empty at their shifted positions.
        assert_eq!(m2.primary, (0, 0));
        assert_eq!(m2.others, vec![(1, 1)]);
    }

    #[test]
    fn delete_drops_bad_and_dedups() {
        let m = Mirrors { primary: (0, 3), others: vec![(0, 3), (8, 11), (9, 20)] };
        let (out, m2) = delete_replicated("foo and foo", &m, 3, 1);
        assert_eq!(out, "fo and fo");
        assert_eq!(m2.others, vec![(7, 9)]);
    }
}
