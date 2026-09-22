pub fn html_to_text(h: &str) -> String {
    let mut out = String::with_capacity(h.len() / 2);
    let mut i = 0;
    let b = h.as_bytes();
    let lower = h.to_ascii_lowercase();
    while i < b.len() {
        if b[i] == b'<' {
            let rest = &lower[i..];
            let skip = ["<style", "<script", "<head"]
                .iter()
                .find(|t| rest.starts_with(*t))
                .map(|t| format!("</{}", &t[1..]));
            if let Some(close) = skip {
                match rest.find(&close) {
                    Some(j) => {
                        i += j + close.len();
                        i += lower[i..].find('>').map(|k| k + 1).unwrap_or(0);
                        continue;
                    }
                    None => break,
                }
            }
            let end = h[i..].find('>').map(|k| i + k + 1).unwrap_or(b.len());
            let tag = &lower[i..end];
            if ["<br", "<p", "</p", "<div", "</div", "<li", "<tr", "<h1", "<h2", "<h3", "<h4", "</h", "<td", "<blockquote"]
                .iter()
                .any(|t| tag.starts_with(t))
            {
                out.push('\n');
            }
            i = end;
        } else if b[i] == b'&' {
            let end = h[i..].find(';').map(|k| i + k + 1).filter(|e| e - i <= 10);
            match end {
                Some(e) => {
                    out.push_str(entity(&h[i..e]));
                    i = e;
                }
                None => {
                    out.push('&');
                    i += 1;
                }
            }
        } else {
            let ch = h[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    squeeze(&out)
}

fn entity(e: &str) -> &str {
    match e {
        "&amp;" => "&",
        "&lt;" => "<",
        "&gt;" => ">",
        "&quot;" => "\"",
        "&#39;" | "&apos;" => "'",
        "&nbsp;" | "&#160;" => " ",
        "&mdash;" => "-",
        "&ndash;" => "-",
        "&hellip;" => "...",
        "&copy;" => "(c)",
        "&zwnj;" | "&zwj;" | "&#8203;" => "",
        _ => "",
    }
}

pub fn squeeze(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut nl = 0;
    let mut sp = false;
    for c in s.chars() {
        if c == '\n' || c == '\r' {
            if nl < 2 {
                out.push('\n');
            }
            nl += 1;
            sp = false;
        } else if c.is_whitespace() {
            if !sp && nl == 0 {
                out.push(' ');
            }
            sp = true;
        } else {
            out.push(c);
            nl = 0;
            sp = false;
        }
    }
    out.trim().to_string()
}

pub fn clip(s: &str, n: usize) -> String {
    if s.len() <= n {
        return s.to_string();
    }
    let mut end = n;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

pub fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn links(h: &str) -> Vec<(String, String)> {
    let lower = h.to_ascii_lowercase();
    let mut out = vec![];
    let mut i = 0;
    while let Some(k) = lower[i..].find("<a") {
        let start = i + k;
        let Some(tag_end) = lower[start..].find('>') else { break };
        let tag_end = start + tag_end;
        let tag = &h[start..=tag_end];
        let href = ["href=\"", "href='"]
            .iter()
            .find_map(|q| {
                let p = tag.to_ascii_lowercase().find(q)? + q.len();
                let quote = q.chars().last().unwrap();
                let e = tag[p..].find(quote)? + p;
                Some(tag[p..e].trim().to_string())
            })
            .unwrap_or_default();
        let close = lower[tag_end..].find("</a").map(|c| tag_end + c).unwrap_or(h.len());
        let txt = html_to_text(&h[tag_end + 1..close]);
        if !href.is_empty() {
            out.push((href.replace("&amp;", "&"), txt));
        }
        i = close.max(tag_end + 1);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_extraction() {
        let h = r#"<p>Hi</p><a href="https://x.com/a?b=1&amp;c=2">Read more</a> <A HREF='https://x.com/u'>Unsubscribe here</A>"#;
        let l = links(h);
        assert_eq!(l.len(), 2);
        assert_eq!(l[0].0, "https://x.com/a?b=1&c=2");
        assert_eq!(l[1], ("https://x.com/u".to_string(), "Unsubscribe here".to_string()));
    }

    #[test]
    fn decode() {
        assert_eq!(pct_decode("unsub%40x.com"), "unsub@x.com");
        assert_eq!(pct_decode("a%2"), "a%2");
    }

    #[test]
    fn html_text() {
        let t = html_to_text("<html><head><style>p{}</style></head><body><p>Hello&nbsp;<b>world</b></p><script>x()</script><p>Bye</p></body></html>");
        assert_eq!(t, "Hello world\n\nBye");
    }
}
