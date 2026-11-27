//! A small, tolerant XML tree shared by the SOAP and DIDL-Lite parsers.
//! Namespace prefixes are dropped (senders disagree on them), so lookups use
//! local names only.

use quick_xml::events::Event;
use quick_xml::Reader;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Element {
    pub name: String,
    pub attrs: Vec<(String, String)>,
    pub children: Vec<Element>,
    pub text: String,
}

impl Element {
    pub fn child(&self, name: &str) -> Option<&Element> {
        self.children.iter().find(|c| c.name.eq_ignore_ascii_case(name))
    }

    pub fn child_text(&self, name: &str) -> Option<&str> {
        self.child(name).map(|c| c.text.as_str())
    }

    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }

    /// First descendant (depth-first, including self) with this local name.
    pub fn find(&self, name: &str) -> Option<&Element> {
        if self.name.eq_ignore_ascii_case(name) {
            return Some(self);
        }
        self.children.iter().find_map(|c| c.find(name))
    }
}

fn local(name: &[u8]) -> String {
    let s = String::from_utf8_lossy(name);
    match s.rsplit_once(':') {
        Some((_, l)) => l.to_string(),
        None => s.into_owned(),
    }
}

/// Parses a document and returns its root element.
pub fn parse(xml: &str) -> Option<Element> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut stack: Vec<Element> = Vec::new();
    let open = |e: &quick_xml::events::BytesStart| Element {
        name: local(e.name().as_ref()),
        attrs: e
            .attributes()
            .flatten()
            .map(|a| {
                let v = a.unescape_value().map(|v| v.into_owned()).unwrap_or_default();
                (local(a.key.as_ref()), v)
            })
            .collect(),
        ..Default::default()
    };
    loop {
        match reader.read_event().ok()? {
            Event::Start(e) => stack.push(open(&e)),
            Event::Empty(e) => {
                let el = open(&e);
                match stack.last_mut() {
                    Some(parent) => parent.children.push(el),
                    None => return Some(el),
                }
            }
            Event::Text(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&t.unescape().ok()?);
                }
            }
            Event::CData(t) => {
                if let Some(top) = stack.last_mut() {
                    top.text.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Event::End(_) => {
                let el = stack.pop()?;
                match stack.last_mut() {
                    Some(parent) => parent.children.push(el),
                    None => return Some(el),
                }
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}

/// Escapes text for element content and attribute values.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_prefixes_and_unescapes() {
        let root = parse(r#"<s:Envelope xmlns:s="x"><s:Body><u:Play><Speed>1</Speed><A>a &amp; b</A></u:Play></s:Body></s:Envelope>"#).unwrap();
        assert_eq!(root.name, "Envelope");
        let play = root.child("Body").unwrap().children.first().unwrap();
        assert_eq!(play.name, "Play");
        assert_eq!(play.child_text("Speed"), Some("1"));
        assert_eq!(play.child_text("A"), Some("a & b"));
    }
}
