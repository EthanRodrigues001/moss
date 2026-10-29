//! Reading `<meta name="ol-…" content="…">` tags from Overleaf pages.
//!
//! Overleaf passes server data to its web app as meta tags whose `content` is HTML-escaped JSON
//! or text. We only need three of them, so a small scanner is enough.

/// The decoded `content` of the `<meta name="{name}">` tag, if present.
pub fn meta_content(html: &str, name: &str) -> Option<String> {
    let needle = format!("name=\"{name}\"");
    let at = html.find(&needle)?;
    let tag_start = html[..at].rfind('<')?;
    let tag = &html[tag_start..tag_end(html, tag_start)];
    attribute_value(tag, "content=").map(unescape)
}

/// Index just past the `>` that closes the tag starting at `start`, ignoring any `>` inside
/// quoted attribute values (JSON content can contain them).
fn tag_end(html: &str, start: usize) -> usize {
    let mut quote: Option<char> = None;
    for (i, c) in html[start..].char_indices() {
        match (quote, c) {
            (None, '"') | (None, '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return start + i + 1,
            _ => {}
        }
    }
    html.len()
}

/// The value of attribute `attr` (written like `content=`) up to its closing quote.
fn attribute_value<'a>(tag: &'a str, attr: &str) -> Option<&'a str> {
    // Require whitespace before the attribute so `data-content=` is not matched.
    let mut search_from = 0;
    let start = loop {
        let found = search_from + tag[search_from..].find(attr)?;
        if tag[..found].ends_with(char::is_whitespace) {
            break found + attr.len();
        }
        search_from = found + attr.len();
    };
    let rest = &tag[start..];
    let quote = rest.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let body = &rest[1..];
    let end = body.find(quote)?;
    Some(&body[..end])
}

pub fn unescape(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#34;", "\"")
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
        .replace("&apos;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_and_unescapes_json_content() {
        let html = r#"<head><meta name="ol-csrfToken" content="abc-123"><meta name="ol-prefetchedProjectsBlob" content="{&quot;projects&quot;:[{&quot;name&quot;:&quot;A &amp; B &gt; C&quot;}]}"></head>"#;
        assert_eq!(meta_content(html, "ol-csrfToken").as_deref(), Some("abc-123"));
        let blob = meta_content(html, "ol-prefetchedProjectsBlob").unwrap();
        assert_eq!(blob, r#"{"projects":[{"name":"A & B > C"}]}"#);
    }

    #[test]
    fn handles_single_quotes_and_reversed_attribute_order() {
        let html = r#"<meta content='x"y' name="ol-a"><meta content="v" name="ol-b">"#;
        assert_eq!(meta_content(html, "ol-a").as_deref(), Some("x\"y"));
        assert_eq!(meta_content(html, "ol-b").as_deref(), Some("v"));
    }

    #[test]
    fn missing_meta_is_none() {
        assert_eq!(meta_content("<html></html>", "ol-csrfToken"), None);
    }
}
