//! Tool versions, compared as semver orders them.

use std::cmp::Ordering;

/// A version such as `0.9.0`, `0.9` (taken as `0.9.0`) or `1.0.0-beta.2`. Build metadata
/// (`+...`) is accepted and ignored, as semver says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Version {
    numbers: [u64; 3],
    /// Pre-release identifiers; empty for a release.
    pre: Vec<Identifier>,
}

/// One dot-separated pre-release identifier. Numeric identifiers sort before alphanumeric ones.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Identifier {
    Number(u64),
    Text(String),
}

impl Version {
    pub(crate) fn parse(text: &str) -> Option<Version> {
        let text = text
            .split_once('+')
            .map_or(text, |(version, _build)| version);
        let (core, pre) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let mut numbers = [0; 3];
        for (index, part) in core.split('.').enumerate() {
            if index == numbers.len()
                || part.is_empty()
                || !part.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            numbers[index] = part.parse().ok()?;
        }
        let pre = match pre {
            None => Vec::new(),
            Some(pre) => pre
                .split('.')
                .map(|part| {
                    if part.is_empty()
                        || !part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
                    {
                        None
                    } else if part.bytes().all(|b| b.is_ascii_digit()) {
                        part.parse().ok().map(Identifier::Number)
                    } else {
                        Some(Identifier::Text(part.to_owned()))
                    }
                })
                .collect::<Option<Vec<_>>>()?,
        };
        Some(Version { numbers, pre })
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Version) -> Ordering {
        self.numbers.cmp(&other.numbers).then_with(|| {
            match (self.pre.is_empty(), other.pre.is_empty()) {
                // A release is newer than any of its pre-releases.
                (true, true) => Ordering::Equal,
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                // Identifier by identifier; a shorter list that matches so far is older.
                (false, false) => self.pre.cmp(&other.pre),
            }
        })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Version) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[cfg(test)]
mod tests {
    use super::Version;

    fn v(text: &str) -> Version {
        Version::parse(text).unwrap_or_else(|| panic!("{text} parses"))
    }

    #[test]
    fn numbers_compare_as_numbers() {
        assert!(v("0.9.0") > v("0.8.9"));
        assert!(v("0.10.0") > v("0.9.0"));
        assert!(v("1.0.0") > v("0.99.99"));
        assert_eq!(v("0.9"), v("0.9.0"));
        assert_eq!(v("1"), v("1.0.0"));
    }

    #[test]
    fn pre_releases_come_before_their_release() {
        assert!(v("0.9.0-beta") < v("0.9.0"));
        assert!(v("0.9.0-beta") > v("0.8.0"));
        assert!(v("1.0.0-alpha") < v("1.0.0-alpha.1"));
        assert!(v("1.0.0-alpha.1") < v("1.0.0-alpha.beta"));
        assert!(v("1.0.0-beta.2") < v("1.0.0-beta.11"));
        assert!(v("1.0.0-beta.11") < v("1.0.0-rc.1"));
        assert!(v("1.0.0-alpha-1") > v("1.0.0-alpha"));
    }

    #[test]
    fn build_metadata_is_ignored() {
        assert_eq!(v("0.9.0+20261104"), v("0.9.0"));
        assert_eq!(v("0.9.0-beta+exp.sha.5114f85"), v("0.9.0-beta"));
    }

    #[test]
    fn what_isnt_a_version() {
        for text in [
            "",
            "v1.0.0",
            "1.",
            ".1",
            "1..2",
            "1.2.3.4",
            "a.b.c",
            "1.2.x",
            "1.2.3-",
            "1.2.3-a..b",
            "1.2.3-a_b",
            "latest",
            " 1.2.3",
        ] {
            assert_eq!(Version::parse(text), None, "{text:?}");
        }
    }
}
