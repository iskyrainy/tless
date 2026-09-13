//! Small helpers shared across modules.

/// Lowercase ASCII slug: runs of non-alphanumeric characters collapse into a
/// single dash. Used for source file names, the `slugify()` template function
/// and heading anchors, so all of them agree.
pub(crate) fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for ch in input.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
            prev_dash = false;
        } else if !prev_dash && !slug.is_empty() {
            slug.push('-');
            prev_dash = true;
        }
    }
    slug.trim_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::slugify;

    #[test]
    fn slugify_normalizes_into_ascii_slugs() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
        assert_eq!(slugify("First Post"), "first-post");
        assert_eq!(slugify("a---b"), "a-b");
        assert_eq!(slugify("-x-"), "x");
        assert_eq!(slugify("café au lait"), "caf-au-lait");
        assert_eq!(slugify(""), "");
    }
}
