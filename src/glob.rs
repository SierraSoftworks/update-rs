//! A tiny, dependency-free glob matcher used to select release assets by name.
//!
//! The pattern is matched against the **entire** name — it is anchored at both
//! ends, never a substring — so an exact pattern like `app-linux-amd64` matches
//! only `app-linux-amd64` and never a sidecar such as `app-linux-amd64.sha256`
//! or `app-linux-amd64.sig`. Two wildcards are supported (there is no path
//! separator handling, because asset names aren't paths):
//!
//! - `*` matches any sequence of characters (including none);
//! - `?` matches exactly one character.
//!
//! Every other character — including `.`, `-`, and `_` — matches literally. To
//! deliberately match a family of names (sidecars included), add an explicit
//! wildcard, e.g. `app-linux-amd64*`.

/// Return `true` if `text` matches the glob `pattern` (`*` and `?` wildcards).
pub(crate) fn matches(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();

    let (mut p, mut t) = (0usize, 0usize);
    // The position of the most recent `*` in the pattern, and the text position
    // we had consumed up to when we matched it — so we can backtrack and let the
    // `*` swallow one more character if the rest of the pattern fails to match.
    let mut star: Option<usize> = None;
    let mut star_t = 0usize;

    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some(p);
            star_t = t;
            p += 1;
        } else if let Some(sp) = star {
            // Backtrack: let the last `*` consume one more character of `text`.
            p = sp + 1;
            star_t += 1;
            t = star_t;
        } else {
            return false;
        }
    }

    // Any trailing `*`s in the pattern can match the empty remainder.
    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }

    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::matches;

    #[test]
    fn literal() {
        assert!(matches("myapp-linux-amd64", "myapp-linux-amd64"));
        assert!(!matches("myapp-linux-amd64", "myapp-linux-arm64"));
        assert!(!matches("myapp-linux-amd64", "myapp-linux-amd64.exe"));
    }

    #[test]
    fn anchored_to_whole_name() {
        // An exact pattern matches only the exact name — never a sidecar file
        // (checksum/signature) or any longer/shorter name that contains it.
        assert!(matches("app-linux-amd64", "app-linux-amd64"));
        assert!(!matches("app-linux-amd64", "app-linux-amd64.sha256"));
        assert!(!matches("app-linux-amd64", "app-linux-amd64.sig"));
        assert!(!matches("app-linux-amd64", "prefix-app-linux-amd64"));
        assert!(!matches("app-linux-amd64", "app-linux-amd6"));
        // A leading/trailing wildcard is required to opt in to matching around it.
        assert!(matches("app-linux-amd64*", "app-linux-amd64.sha256"));
        assert!(matches("*app-linux-amd64", "prefix-app-linux-amd64"));
    }

    #[test]
    fn star() {
        assert!(matches("*", "anything"));
        assert!(matches("*-linux-amd64", "myapp-linux-amd64"));
        assert!(matches("myapp-*-amd64", "myapp-linux-amd64"));
        assert!(matches("myapp-*", "myapp-windows-amd64.exe"));
        assert!(matches("myapp-linux-amd64*", "myapp-linux-amd64"));
        assert!(!matches("*-linux-amd64", "myapp-linux-arm64"));
    }

    #[test]
    fn question_mark() {
        assert!(matches("v?.?.?", "v1.2.3"));
        assert!(matches("myapp-linux-amd6?", "myapp-linux-amd64"));
        assert!(!matches("myapp-linux-amd6?", "myapp-linux-amd644"));
        assert!(!matches("?", ""));
    }

    #[test]
    fn empty() {
        assert!(matches("", ""));
        assert!(!matches("", "x"));
        assert!(matches("**", ""));
    }
}
