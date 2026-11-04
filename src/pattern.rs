//! Glob patterns: profile `names` and `paths`, and branch rules.

use globset::{GlobBuilder, GlobMatcher};
use serde::Deserialize;

/// A glob from `rules.json`: one of a profile's `names` or `paths`, or a branch rule's `pattern`.
///
/// Every pattern is compiled the same way, on every platform, so every caller gets the same
/// answer:
/// - matching is case-sensitive;
/// - `*`, `?` and `[...]` never match `/`, and `**` matches across folders;
/// - `\` escapes the next character (globset does this by default only on Unix).
///
/// A pattern that isn't a valid glob never matches anything; [`Rules::check`](crate::Rules::check)
/// reports it.
#[derive(Clone, Debug, Deserialize)]
#[serde(from = "String")]
pub struct Pattern {
    text: String,
    glob: Result<GlobMatcher, String>,
}

impl From<String> for Pattern {
    fn from(text: String) -> Pattern {
        let glob = GlobBuilder::new(&text)
            .case_insensitive(false)
            .literal_separator(true)
            .backslash_escape(true)
            .build()
            .map(|glob| glob.compile_matcher())
            .map_err(|error| error.kind().to_string());
        Pattern { text, glob }
    }
}

impl From<&str> for Pattern {
    fn from(text: &str) -> Pattern {
        Pattern::from(text.to_owned())
    }
}

impl PartialEq for Pattern {
    fn eq(&self, other: &Pattern) -> bool {
        self.text == other.text
    }
}

impl Eq for Pattern {}

impl Pattern {
    /// The pattern as written in `rules.json`.
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// Whether `candidate` (a file name without its extension, a repository-relative path with
    /// `/` separators, or a branch name) matches. Always `false` for an invalid pattern.
    pub fn is_match(&self, candidate: &str) -> bool {
        self.glob
            .as_ref()
            .is_ok_and(|glob| glob.is_match(candidate))
    }

    /// Why the pattern isn't a valid glob, or `None` if it is.
    pub fn error(&self) -> Option<&str> {
        self.glob.as_ref().err().map(String::as_str)
    }

    /// Whether, as a `names` pattern, this matches every file name `other` matches.
    ///
    /// `true` is a proof; `false` only means it couldn't be shown (see [`tokens_cover`]).
    pub(crate) fn covers_name(&self, other: &Pattern) -> bool {
        if self.error().is_some() || other.error().is_some() {
            return false;
        }
        // File names (without extension) contain no `/`, so these match every one.
        self.text == "*" || self.text == "**" || self.covers_by_tokens(other)
    }

    /// Whether, as a `paths` pattern, this matches every path `other` matches.
    ///
    /// `true` is a proof; `false` only means it couldn't be shown (see [`tokens_cover`]).
    pub(crate) fn covers_path(&self, other: &Pattern) -> bool {
        if self.error().is_some() || other.error().is_some() {
            return false;
        }
        if self.text == "**" {
            return true;
        }
        // `Assets/Art/**` covers anything that starts with the literal text `Assets/Art/`, since
        // every path such a pattern matches lies inside that folder.
        if let Some(folder) = self.text.strip_suffix("/**")
            && !folder.contains(['*', '?', '[', '{', '\\'])
            && other.text.starts_with(&format!("{folder}/"))
        {
            return true;
        }
        self.covers_by_tokens(other)
    }

    fn covers_by_tokens(&self, other: &Pattern) -> bool {
        if self.text == other.text {
            return true;
        }
        match (tokens(&self.text), tokens(&other.text)) {
            (Some(p), Some(q)) => tokens_cover(&p, &q),
            _ => false,
        }
    }
}

/// One piece of a pattern, for [`tokens_cover`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Token<'a> {
    /// A literal character.
    Lit(char),
    /// `?`: any one character except `/`.
    One,
    /// `*`: any run of characters without `/`.
    Star,
    /// `[...]`: one character from a set. `may_slash` is set when the set could include `/`
    /// (it names `/`, is negated, or has a range), so it isn't treated as "never `/`".
    Class { text: &'a str, may_slash: bool },
}

/// Splits a pattern into tokens, or `None` if it uses syntax the covering test doesn't reason
/// about (`**`, `{a,b}` alternatives, `\` inside a class).
fn tokens(text: &str) -> Option<Vec<Token<'_>>> {
    let mut out = Vec::new();
    let mut chars = text.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        match c {
            '*' if matches!(chars.peek(), Some((_, '*'))) => return None,
            '*' => out.push(Token::Star),
            '?' => out.push(Token::One),
            '{' | '}' => return None,
            '\\' => out.push(Token::Lit(chars.next()?.1)),
            '[' => {
                let mut may_slash = false;
                let mut first = true;
                let end = loop {
                    let (at, c) = chars.next()?;
                    match c {
                        '!' | '^' if first => may_slash = true,
                        ']' if !first => break at,
                        '\\' => return None,
                        '/' | '-' => {
                            may_slash = true;
                            first = false;
                        }
                        _ => first = false,
                    }
                };
                out.push(Token::Class {
                    text: &text[start..=end],
                    may_slash,
                });
            }
            c => out.push(Token::Lit(c)),
        }
    }
    Some(out)
}

/// Whether every string `q` matches is also matched by `p`.
///
/// Sound but not complete: `true` is a proof; `false` means it couldn't be shown. `p`'s `*` may
/// take any run of `q`'s tokens that can only stand for text without `/`; `p`'s `?` may take one
/// such token other than `*`; `p`'s literals and classes must meet the identical token in `q`.
fn tokens_cover(p: &[Token<'_>], q: &[Token<'_>]) -> bool {
    match p.split_first() {
        None => q.is_empty(),
        Some((Token::Star, p_rest)) => {
            let mut taken = 0;
            loop {
                if tokens_cover(p_rest, &q[taken..]) {
                    return true;
                }
                match q.get(taken) {
                    Some(&token) if no_slash(token) => taken += 1,
                    _ => return false,
                }
            }
        }
        Some((Token::One, p_rest)) => match q.split_first() {
            Some((&token, q_rest)) if token != Token::Star && no_slash(token) => {
                tokens_cover(p_rest, q_rest)
            }
            _ => false,
        },
        Some((token, p_rest)) => match q.split_first() {
            Some((other, q_rest)) if other == token => tokens_cover(p_rest, q_rest),
            _ => false,
        },
    }
}

/// Whether `token` can only ever stand for text that contains no `/`.
fn no_slash(token: Token<'_>) -> bool {
    match token {
        Token::Lit(c) => c != '/',
        Token::One | Token::Star => true,
        Token::Class { may_slash, .. } => !may_slash,
    }
}

#[cfg(test)]
mod tests {
    use super::Pattern;

    fn name_covers(p: &str, q: &str) -> bool {
        Pattern::from(p).covers_name(&Pattern::from(q))
    }

    fn path_covers(p: &str, q: &str) -> bool {
        Pattern::from(p).covers_path(&Pattern::from(q))
    }

    #[test]
    fn matching_is_case_sensitive_and_stars_stay_in_one_folder() {
        assert!(Pattern::from("T_*_N").is_match("T_Crate_N"));
        assert!(!Pattern::from("T_*_N").is_match("t_crate_n"));
        assert!(!Pattern::from("Assets/*").is_match("Assets/Art/T_Crate.png"));
        assert!(Pattern::from("Assets/**").is_match("Assets/Art/T_Crate.png"));
        assert!(Pattern::from("proto/*").is_match("proto/crates"));
        assert!(!Pattern::from("proto/*").is_match("proto/crates/v2"));
    }

    #[test]
    fn an_invalid_pattern_reports_why_and_never_matches() {
        let pattern = Pattern::from("T_[");
        assert!(pattern.error().is_some());
        assert!(!pattern.is_match("T_["));
        assert_eq!(Pattern::from("T_*").error(), None);
    }

    #[test]
    fn a_wider_name_covers_a_narrower_one() {
        assert!(name_covers("*", "SM_[AB]{x,y}"));
        assert!(name_covers("T_*", "T_*_N"));
        assert!(name_covers("T_*", "T_Crate"));
        assert!(name_covers("T_*", "T_?"));
        assert!(name_covers("T_*_N", "T_Crate_N"));
        assert!(name_covers("*_N", "T_*_N"));
        assert!(name_covers("T_?_N", "T_[AB]_N"));
        assert!(name_covers("T_[AB]_N", "T_[AB]_N"));
        assert!(name_covers("T_\\*", "T_\\*"));
    }

    #[test]
    fn a_name_that_matches_more_is_not_covered() {
        assert!(!name_covers("T_*_N", "T_*"));
        assert!(!name_covers("T_?", "T_*"));
        assert!(!name_covers("SK_*", "SM_*"));
        assert!(!name_covers("T_*", "t_*"));
        assert!(!name_covers("T_[AB]", "T_[ABC]"));
        assert!(!name_covers("T_[", "T_["));
    }

    #[test]
    fn a_wider_path_covers_a_narrower_one() {
        assert!(path_covers("**", "Assets/Art/*"));
        assert!(path_covers("Assets/Art/**", "Assets/Art/Environment/**"));
        assert!(path_covers("Assets/Art/**", "Assets/Art/{Props,Rocks}/*"));
        assert!(path_covers("Assets/*/T_*", "Assets/Art/T_*"));
        assert!(path_covers("Assets/Art/**", "Assets/Art/**"));
    }

    #[test]
    fn a_path_that_matches_more_is_not_covered() {
        assert!(!path_covers("Assets/Art/Environment/**", "Assets/Art/**"));
        assert!(!path_covers("Assets/*", "Assets/**"));
        assert!(!path_covers("Assets/*", "Assets/Art/*"));
        assert!(!path_covers("Assets/Art/**", "Assets/Artwork/**"));
        assert!(!path_covers("Assets/*", "Assets/[+-0]x"));
    }
}
