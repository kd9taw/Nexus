//! What a connector's read-only Test learned from the service's answer, and the plain text of an
//! HTML answer page. Pure: the requests are built by each connector's module ([`crate::lotw`],
//! [`crate::eqsl`], [`crate::clublog`]) and sent by the thin transports behind the `live` feature.
//!
//! A Test signs in and reads; it never uploads and never changes anything on the service.

/// The verdict a Test reads off a service's answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoginCheck {
    /// The service took the credentials and answered the read.
    Accepted,
    /// The service refused them, in its own words.
    Refused(String),
    /// The service answered with something that is neither, so the Test cannot tell. Its words,
    /// when the answer had any that could be read.
    Unrecognised(Option<String>),
}

/// The longest run of a service's words a Test passes on: a refusal is a sentence, and a page
/// that is not one should not fill the Settings panel.
const MAX_WORDS: usize = 200;

/// The visible lines of an HTML page: tags dropped, the contents of `<script>` and `<style>`
/// dropped, the common entities decoded, whitespace collapsed, empty lines left out.
pub fn page_lines(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut text = String::with_capacity(html.len());
    let mut at = 0;
    while let Some(open) = lower[at..].find('<').map(|i| at + i) {
        text.push_str(&html[at..open]);
        let Some(close) = lower[open..].find('>').map(|i| open + i) else {
            at = html.len();
            break;
        };
        let tag = &lower[open + 1..close];
        let skip_to = ["script", "style"].iter().find_map(|name| {
            let opens = tag.starts_with(name)
                && tag[name.len()..]
                    .chars()
                    .next()
                    .is_none_or(|c| c.is_whitespace() || c == '>' || c == '/');
            opens.then(|| {
                let end = format!("</{name}");
                lower[close..]
                    .find(&end)
                    .and_then(|i| lower[close + i..].find('>').map(|j| close + i + j))
                    .unwrap_or(html.len() - 1)
            })
        });
        // Every tag ends a line: a heading or a cell is its own sentence.
        text.push('\n');
        at = skip_to.unwrap_or(close) + 1;
    }
    if at < html.len() {
        text.push_str(&html[at..]);
    }
    text.lines()
        .map(|line| {
            line.replace("&nbsp;", " ")
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&#39;", "'")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|line| !line.is_empty())
        .collect()
}

/// `words` cut to [`MAX_WORDS`] characters, on a character boundary.
pub fn capped(words: &str) -> String {
    let words = words.trim();
    match words.char_indices().nth(MAX_WORDS) {
        Some((cut, _)) => format!("{}…", words[..cut].trim_end()),
        None => words.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_lines_keep_the_visible_words_and_drop_scripts() {
        let html = "<html><head><title>Download InBox - eQSL.cc</title>\
            <script>function f(){ if (a<b) alert('Error: not this'); }</script>\
            <style>h3{color:red}</style></head>\
            <body><H3>Error: No such   Callsign found</H3><p>Tom &amp; Jerry</p></body></html>";
        assert_eq!(
            page_lines(html),
            vec![
                "Download InBox - eQSL.cc",
                "Error: No such Callsign found",
                "Tom & Jerry"
            ]
        );
    }

    /// Control: a tag that only starts with "script" is not a script, and its text is kept.
    #[test]
    fn page_lines_keep_a_tag_that_only_starts_like_script() {
        assert_eq!(
            page_lines("<scripted>shown</scripted>"),
            vec!["shown".to_string()]
        );
    }

    #[test]
    fn capped_cuts_long_words_on_a_char_boundary() {
        assert_eq!(capped("  short  "), "short");
        let long = "é".repeat(MAX_WORDS + 5);
        let cut = capped(&long);
        assert_eq!(cut.chars().count(), MAX_WORDS + 1);
        assert!(cut.ends_with('…'));
    }
}
