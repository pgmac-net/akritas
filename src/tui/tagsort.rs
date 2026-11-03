//! Version-aware ordering for the Tags pane (issue #160).
//!
//! Plain lexical order puts `0.10.0` before `0.2.0`, which is never what
//! anyone browsing chart or image versions wants. Tags that look like a
//! version are ordered by SemVer precedence instead; everything else
//! (`latest`, `main`, `sha-…`) stays lexical and is grouped after the
//! versions.

use semver::Version;

/// Read `tag` as a version, leniently.
///
/// Real registries do not use strict SemVer, so before handing the tag to the
/// `semver` crate this:
/// - strips one leading `v`/`V` (`v1.2.3`);
/// - pads a one- or two-part core to three (`1.25` → `1.25.0`);
/// - reads the first `_` as `+` — Helm stores build metadata (`1.2.3+build5`)
///   as `1.2.3_build5` because `+` is not a legal OCI tag character.
///
/// Returns `None` for anything that still is not a version, e.g. `latest`,
/// `sha-abc123`, a four-part `1.2.3.4`, or a core with leading zeros.
pub fn version_key(tag: &str) -> Option<Version> {
    let tag = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
    if !tag.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }

    // `_` → `+` only for the first one, and only past the core: a prerelease
    // identifier may not contain `_`, so that tag is not a version anyway.
    let normalised = tag.replacen('_', "+", 1);

    // Core is everything before the first `-` (prerelease) or `+` (build).
    let split = normalised.find(['-', '+']).unwrap_or(normalised.len());
    let (core, rest) = normalised.split_at(split);

    let parts: Vec<&str> = core.split('.').collect();
    if parts.is_empty() || parts.len() > 3 {
        return None;
    }
    let mut padded = parts.join(".");
    for _ in parts.len()..3 {
        padded.push_str(".0");
    }

    Version::parse(&format!("{padded}{rest}")).ok()
}

/// Sort `tags` in place.
///
/// Version tags come first, ordered by SemVer precedence (so `1.0.0-rc.1`
/// precedes `1.0.0` and `rc.2` precedes `rc.10`); ties — `v1.2.3` against
/// `1.2.3`, or differing build metadata — break lexically so the order is
/// deterministic. Other tags follow, lexically.
///
/// `newest_first` reverses **only** the version group: the point of a
/// descending sort is the newest version on top, and floating tags such as
/// `latest` should stay grouped at the end rather than jump to the top.
pub fn sort_tags(tags: &mut Vec<String>, newest_first: bool) {
    let mut versions: Vec<(Version, String)> = Vec::new();
    let mut others: Vec<String> = Vec::new();
    for tag in tags.drain(..) {
        match version_key(&tag) {
            Some(v) => versions.push((v, tag)),
            None => others.push(tag),
        }
    }

    versions.sort_by(|(va, ta), (vb, tb)| va.cmp_precedence(vb).then_with(|| ta.cmp(tb)));
    others.sort();
    if newest_first {
        versions.reverse();
    }

    tags.extend(versions.into_iter().map(|(_, tag)| tag));
    tags.extend(others);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(input: &[&str], newest_first: bool) -> Vec<String> {
        let mut tags: Vec<String> = input.iter().map(|s| (*s).to_owned()).collect();
        sort_tags(&mut tags, newest_first);
        tags
    }

    #[test]
    fn version_key_accepts_the_shapes_registries_really_use() {
        for tag in [
            "0.10.1",
            "2.2.0-rc.1",
            "1.0.0-beta2",
            "v1.2.3",
            "V1.2.3",
            "1.25",
            "7",
            "1.2.3_build5",
            "1.2.3-rc.1_build5",
        ] {
            assert!(version_key(tag).is_some(), "{tag} should be a version");
        }
    }

    #[test]
    fn version_key_rejects_everything_else() {
        for tag in [
            "latest",
            "main",
            "sha-abc123",
            "sha256:deadbeef",
            "1.2.3.4",
            "01.2.3",
            "v",
            "",
            "-1.0.0",
            "1.2.x",
            "1..2",
            "release-1.2.3",
        ] {
            assert!(version_key(tag).is_none(), "{tag} should not be a version");
        }
    }

    #[test]
    fn version_key_normalises() {
        assert_eq!(version_key("v1.25"), Version::parse("1.25.0").ok());
        assert_eq!(version_key("7"), Version::parse("7.0.0").ok());
        assert_eq!(
            version_key("1.2.3_build5"),
            Version::parse("1.2.3+build5").ok()
        );
    }

    /// The motivating case: lexical order interleaves 0.10.x with 0.2.x.
    #[test]
    fn numeric_parts_compare_as_numbers() {
        let tags = ["0.9.3", "0.10.0", "0.2.0", "0.15.1", "0.3.0"];
        assert_eq!(
            sorted(&tags, false),
            ["0.2.0", "0.3.0", "0.9.3", "0.10.0", "0.15.1"]
        );
        assert_eq!(
            sorted(&tags, true),
            ["0.15.1", "0.10.0", "0.9.3", "0.3.0", "0.2.0"]
        );
    }

    #[test]
    fn prerelease_precedence_follows_the_spec() {
        let expected = [
            "1.0.0-alpha",
            "1.0.0-alpha.1",
            "1.0.0-beta",
            "1.0.0-rc.1",
            "1.0.0-rc.2",
            "1.0.0-rc.10",
            "1.0.0",
        ];
        // Feed them reversed so the sort has real work to do.
        let mut shuffled: Vec<&str> = expected.to_vec();
        shuffled.reverse();
        assert_eq!(sorted(&shuffled, false), expected);
    }

    /// `latest` and friends stay lexical and last, in both directions; only
    /// the versions flip.
    #[test]
    fn non_version_tags_are_grouped_last_in_both_directions() {
        let tags = ["latest", "main", "1.2.0", "sha-abc", "0.9.0"];
        assert_eq!(
            sorted(&tags, false),
            ["0.9.0", "1.2.0", "latest", "main", "sha-abc"]
        );
        assert_eq!(
            sorted(&tags, true),
            ["1.2.0", "0.9.0", "latest", "main", "sha-abc"]
        );
    }

    /// Equal precedence must still order deterministically.
    #[test]
    fn ties_break_lexically() {
        let a = sorted(&["v1.2.3", "1.2.3", "1.2.3_b2", "1.2.3_b1"], false);
        let b = sorted(&["1.2.3_b1", "1.2.3", "v1.2.3", "1.2.3_b2"], false);
        assert_eq!(a, b, "input order must not change the result");
        assert_eq!(a, ["1.2.3", "1.2.3_b1", "1.2.3_b2", "v1.2.3"]);
    }

    #[test]
    fn empty_and_single_lists_are_fine() {
        assert!(sorted(&[], true).is_empty());
        assert_eq!(sorted(&["only"], false), ["only"]);
    }
}
