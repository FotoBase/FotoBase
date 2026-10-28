//! XMP sidecar read/write with merge-on-write semantics.
//!
//! Implements `docs/sidecar-merge-on-write.md` §3: parse the full XMP tree as
//! it exists on disk right now, modify only known nodes, serialize, back up
//! the previous version, then atomically replace via `.tmp` + rename.
//!
//! The exiv2 backend (rexiv2 fork) is not linked yet; this module uses
//! quick-xml for full-tree parse/serialize so unknown nodes, comments and
//! attributes survive verbatim. Once the rexiv2 fork lands the same
//! [`SidecarBackend`] seam swaps to it (PLAN §10).

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use quick_xml::events::{BytesDecl, Event};
use quick_xml::Reader;
use quick_xml::XmlVersion;

/// The Fotobase XMP namespace URI (00-010 §2).
pub const FB_NS: &str = "https://fotobase.github.io/ns/1.0/";

/// Re-exported for callers constructing `fb:` elements.
pub type FbNs = &'static str;

/// Crate-internal seam so the exiv2 backend (rexiv2 fork, PLAN §10) can be
/// swapped without touching callers. The quick-xml tree implementation is the
/// current backend; the rexiv2-backed one must satisfy the same M1–M3
/// properties (unknown fields preserved, backup, atomic replace).
pub trait SidecarBackend {
    fn read(&self, path: &Path) -> Result<Sidecar, SidecarError>;
    fn write(&self, sidecar: &Sidecar, path: &Path) -> Result<(), SidecarError>;
}

/// Current backend: pure-Rust XMP tree via quick-xml.
#[derive(Debug, Default, Clone, Copy)]
pub struct QuickXmlBackend;

impl SidecarBackend for QuickXmlBackend {
    fn read(&self, path: &Path) -> Result<Sidecar, SidecarError> {
        Sidecar::read(path)
    }
    fn write(&self, sidecar: &Sidecar, path: &Path) -> Result<(), SidecarError> {
        sidecar.write(path)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SidecarError {
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("malformed XMP sidecar {path}: {message}")]
    Malformed { path: PathBuf, message: String },
}

impl SidecarError {
    fn malformed(path: &Path, message: impl Into<String>) -> Self {
        SidecarError::Malformed { path: path.to_path_buf(), message: message.into() }
    }
}

/// XMP packet parsed as a full tree. Unknown nodes (any element we don't
/// interpret) are kept verbatim, including comments and attributes.
#[derive(Debug, Default, Clone)]
pub struct XmpTree {
    /// Raw prolog bytes before the root element (XML decl, doctype).
    prolog: Vec<u8>,
    root: Option<Element>,
    /// Trailing bytes after the root (trailing xpacket PI, whitespace).
    epilog: Vec<u8>,
}

#[derive(Debug, Clone)]
struct Element {
    prefix: String,
    local: String,
    attributes: Vec<(String, String)>,
    children: Vec<Child>,
}

#[derive(Debug, Clone)]
enum Child {
    Element(Element),
    Text(String),
    Comment(String),
    Pi(String),
    /// Text that was in the original source but is only whitespace — kept so
    /// serialization stays byte-stable where possible.
    Whitespace(String),
}

impl Element {
    fn name(&self) -> String {
        if self.prefix.is_empty() {
            self.local.clone()
        } else {
            format!("{}:{}", self.prefix, self.local)
        }
    }

    fn find_child(&self, prefix: &str, local: &str) -> Option<&Element> {
        self.children.iter().find_map(|c| match c {
            Child::Element(e) if e.prefix == prefix && e.local == local => Some(e),
            _ => None,
        })
    }

    fn find_child_mut(&mut self, prefix: &str, local: &str) -> Option<&mut Element> {
        self.children.iter_mut().find_map(|c| match c {
            Child::Element(e) if e.prefix == prefix && e.local == local => Some(e),
            _ => None,
        })
    }

    fn child_text(&self, prefix: &str, local: &str) -> Option<String> {
        self.children.iter().find_map(|c| match c {
            Child::Element(e) if e.prefix == prefix && e.local == local => {
                let mut text = String::new();
                for ch in &e.children {
                    if let Child::Text(t) = ch {
                        text.push_str(t);
                    }
                }
                Some(text)
            }
            _ => None,
        })
    }

    fn attr(&self, name: &str) -> Option<&str> {
        self.attributes.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    /// Simple property: child element text, or the attribute form
    /// (`xmp:Rating="4"`) darktable/exiv2/digiKam all emit on
    /// rdf:Description. Non-empty child text wins; empty text falls through.
    fn child_text_or_attr(&self, prefix: &str, local: &str) -> Option<String> {
        let attr_name = format!("{prefix}:{local}");
        match self.child_text(prefix, local) {
            Some(t) if !t.trim().is_empty() => Some(t),
            _ => self.attr(&attr_name).map(str::to_string),
        }
    }

    /// Language-alternative property (`dc:description`): the `x-default`
    /// `rdf:li` text of the `rdf:Alt` child, else the first non-empty one;
    /// falls back to direct text, then the attribute form.
    fn child_alt_text(&self, prefix: &str, local: &str) -> Option<String> {
        let attr_name = format!("{prefix}:{local}");
        if let Some(node) = self.find_child(prefix, local) {
            if let Some(alt) = node.find_child("rdf", "Alt") {
                let items: Vec<(&Element, String)> = alt
                    .children
                    .iter()
                    .filter_map(|c| match c {
                        Child::Element(li) if li.prefix == "rdf" && li.local == "li" => {
                            Some((li, li.direct_text()))
                        }
                        _ => None,
                    })
                    .filter(|(_, text)| !text.trim().is_empty())
                    .collect();
                let default = items.iter().find(|(li, _)| li.attr("xml:lang") == Some("x-default"));
                if let Some((_, text)) = default.or_else(|| items.first()) {
                    return Some(text.clone());
                }
            }
            let direct = node.direct_text();
            if !direct.trim().is_empty() {
                return Some(direct);
            }
        }
        self.attr(&attr_name).map(str::to_string)
    }

    fn direct_text(&self) -> String {
        self.children
            .iter()
            .filter_map(|c| match c {
                Child::Text(t) => Some(t.as_str()),
                _ => None,
            })
            .collect()
    }

    /// Set the `x-default` entry of a language-alternative property, the
    /// form darktable/digiKam/exiv2 read for `dc:description`. Other
    /// languages in an existing `rdf:Alt` are kept; the legacy plain-text and
    /// attribute forms are upgraded in place (no duplicate value).
    fn set_alt_default(&mut self, prefix: &str, local: &str, text: &str) {
        self.attributes.retain(|(k, _)| *k != format!("{prefix}:{local}"));
        let li = || Element {
            prefix: "rdf".to_string(),
            local: "li".to_string(),
            attributes: vec![("xml:lang".to_string(), "x-default".to_string())],
            children: vec![Child::Text(text.to_string())],
        };
        let new_alt = || Element {
            prefix: "rdf".to_string(),
            local: "Alt".to_string(),
            attributes: Vec::new(),
            children: vec![Child::Element(li())],
        };
        let Some(node) = self.find_child_mut(prefix, local) else {
            self.children.push(Child::Element(Element {
                prefix: prefix.to_string(),
                local: local.to_string(),
                attributes: Vec::new(),
                children: vec![Child::Element(new_alt())],
            }));
            return;
        };
        match node.find_child_mut("rdf", "Alt") {
            Some(alt) => {
                let existing = alt.children.iter_mut().find_map(|c| match c {
                    Child::Element(e)
                        if e.prefix == "rdf"
                            && e.local == "li"
                            && e.attr("xml:lang") == Some("x-default") =>
                    {
                        Some(e)
                    }
                    _ => None,
                });
                match existing {
                    Some(e) => {
                        e.children.clear();
                        e.children.push(Child::Text(text.to_string()));
                    }
                    // x-default goes first, as the XMP spec asks.
                    None => alt.children.insert(0, Child::Element(li())),
                }
            }
            None => {
                node.children.clear();
                node.children.push(Child::Element(new_alt()));
            }
        }
    }

    /// Drop a simple property in both forms: child element and attribute.
    fn remove_prop(&mut self, prefix: &str, local: &str) {
        let attr_name = format!("{prefix}:{local}");
        self.attributes.retain(|(k, _)| *k != attr_name);
        self.children
            .retain(|c| !matches!(c, Child::Element(e) if e.prefix == prefix && e.local == local));
    }

    fn set_or_replace_child(&mut self, prefix: &str, local: &str, text: &str) {
        // Replace an existing node in place to keep tree order stable.
        for c in &mut self.children {
            if let Child::Element(e) = c {
                if e.prefix == prefix && e.local == local {
                    e.children.clear();
                    e.children.push(Child::Text(text.to_string()));
                    return;
                }
            }
        }
        self.children.push(Child::Element(Element {
            prefix: prefix.to_string(),
            local: local.to_string(),
            attributes: Vec::new(),
            children: vec![Child::Text(text.to_string())],
        }));
    }

    fn set_or_replace_bag(&mut self, prefix: &str, local: &str, items: &[String]) {
        // Rebuild as an explicit rdf:Bag — the form every mainstream tool
        // (exiv2/digiKam, darktable) reads without parseType tricks.
        let mut bag = Element {
            prefix: "rdf".to_string(),
            local: "Bag".to_string(),
            attributes: Vec::new(),
            children: Vec::new(),
        };
        for item in items {
            bag.children.push(Child::Element(Element {
                prefix: "rdf".to_string(),
                local: "li".to_string(),
                attributes: Vec::new(),
                children: vec![Child::Text(item.clone())],
            }));
        }
        if let Some(existing) = self.find_child_mut(prefix, local) {
            // Replace in place to keep tree order stable.
            existing.children.clear();
            existing.children.push(Child::Element(bag));
            return;
        }
        self.children.push(Child::Element(Element {
            prefix: prefix.to_string(),
            local: local.to_string(),
            attributes: Vec::new(),
            children: vec![Child::Element(bag)],
        }));
    }

    fn bag_items(&self, prefix: &str, local: &str) -> Vec<String> {
        let mut out = Vec::new();
        let Some(node) = self.find_child(prefix, local) else {
            return out;
        };
        // Explicit rdf:Bag wrapper (our writes, digiKam, darktable)…
        for c in &node.children {
            if let Child::Element(bag) = c {
                for li in &bag.children {
                    append_li(li, &mut out);
                }
            }
        }
        // …or rdf:parseType="Bag" with direct rdf:li children.
        if out.is_empty() {
            for li in &node.children {
                append_li(li, &mut out);
            }
        }
        out
    }
}

fn append_li(child: &Child, out: &mut Vec<String>) {
    if let Child::Element(li_el) = child {
        if li_el.prefix == "rdf" && li_el.local == "li" {
            let mut text = String::new();
            for ch in &li_el.children {
                if let Child::Text(t) = ch {
                    text.push_str(t);
                }
            }
            out.push(text);
        }
    }
}

impl XmpTree {
    pub fn parse(bytes: &[u8], path: &Path) -> Result<Self, SidecarError> {
        let mut reader = Reader::from_reader(bytes);
        reader.config_mut().trim_text(false);
        reader.config_mut().expand_empty_elements = true;
        reader.config_mut().check_comments = false;

        let mut prolog: Vec<u8> = Vec::new();
        let mut root: Option<Element> = None;
        let mut stack: Vec<Element> = Vec::new();
        let mut epilog: Vec<u8> = Vec::new();

        loop {
            match reader.read_event() {
                Ok(Event::Start(e)) => {
                    let name = e.name().as_ref().to_string();
                    let (prefix, local) = split_name(&name);
                    let attributes = e
                        .attributes()
                        .filter_map(|a| a.ok())
                        .map(|a| {
                            (
                                a.key.as_ref().to_string(),
                                a.normalized_value(XmlVersion::Implicit1_0)
                                    .unwrap_or_default()
                                    .into_owned(),
                            )
                        })
                        .collect();
                    let el = Element { prefix, local, attributes, children: Vec::new() };
                    stack.push(el);
                }
                Ok(Event::Empty(e)) => {
                    let name = e.name().as_ref().to_string();
                    let (prefix, local) = split_name(&name);
                    let attributes = e
                        .attributes()
                        .filter_map(|a| a.ok())
                        .map(|a| {
                            (
                                a.key.as_ref().to_string(),
                                a.normalized_value(XmlVersion::Implicit1_0)
                                    .unwrap_or_default()
                                    .into_owned(),
                            )
                        })
                        .collect();
                    let el = Element { prefix, local, attributes, children: Vec::new() };
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(Child::Element(el));
                    } else if root.is_none() {
                        root = Some(el);
                    }
                }
                Ok(Event::End(_)) => {
                    let el = stack
                        .pop()
                        .ok_or_else(|| SidecarError::malformed(path, "unbalanced end tag"))?;
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(Child::Element(el));
                    } else {
                        if root.is_some() {
                            return Err(SidecarError::malformed(path, "multiple root elements"));
                        }
                        root = Some(el);
                    }
                }
                Ok(Event::Text(t)) => {
                    let text = t.xml_content(XmlVersion::Implicit1_0).into_owned();
                    if let Some(parent) = stack.last_mut() {
                        if text.trim().is_empty() && parent.children.is_empty() {
                            parent.children.push(Child::Whitespace(text));
                        } else {
                            push_text(parent, &text);
                        }
                    }
                }
                Ok(Event::CData(t)) => {
                    let text = t.as_ref().to_string();
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(Child::Text(text));
                    }
                }
                Ok(Event::Comment(c)) => {
                    let text = c.as_ref().to_string();
                    let child = Child::Comment(text);
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(child);
                    }
                }
                Ok(Event::PI(p)) => {
                    let text = p.as_ref().to_string();
                    if stack.is_empty() && root.is_some() {
                        epilog.extend_from_slice(format!("<?{text}?>\n").as_bytes());
                    } else if let Some(parent) = stack.last_mut() {
                        parent.children.push(Child::Pi(text));
                    }
                }
                Ok(Event::DocType(d)) => prolog.extend_from_slice(d.as_ref().as_bytes()),
                Ok(Event::Decl(_)) => {}
                Ok(Event::Eof) => break,
                Err(err) => return Err(SidecarError::malformed(path, format!("{err}"))),
                // `&amp;`, `&#10;`, … arrive as their own events; dropping them
                // would silently eat characters from values on the next write.
                Ok(Event::GeneralRef(r)) => {
                    let resolved = match r.resolve_char_ref() {
                        Ok(Some(ch)) => ch.to_string(),
                        _ => match &*r {
                            "lt" => "<".to_string(),
                            "gt" => ">".to_string(),
                            "amp" => "&".to_string(),
                            "quot" => "\"".to_string(),
                            "apos" => "'".to_string(),
                            other => format!("&{other};"),
                        },
                    };
                    if let Some(parent) = stack.last_mut() {
                        push_text(parent, &resolved);
                    }
                }
            }
        }

        if !stack.is_empty() {
            return Err(SidecarError::malformed(path, "unclosed element(s)"));
        }
        if root.is_none() && !bytes.is_empty() {
            return Err(SidecarError::malformed(path, "no root element"));
        }
        Ok(XmpTree { prolog, root, epilog })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if !self.prolog.is_empty() {
            out.extend_from_slice(&self.prolog);
            out.push(b'\n');
        } else {
            let decl = BytesDecl::new("1.0", Some("UTF-8"), None);
            out.extend_from_slice(b"<?");
            out.extend_from_slice(decl.as_ref().as_bytes());
            out.extend_from_slice(b"?>\n");
        }
        if let Some(root) = &self.root {
            write_element(&mut out, root, 0);
        }
        out.extend_from_slice(&self.epilog);
        out
    }

    fn description_mut(&mut self) -> Option<&mut Element> {
        let root = self.root.as_mut()?;
        // Descend x:xmpmeta > rdf:RDF > rdf:Description (first one is ours).
        if root.local == "xmpmeta" {
            let rdf = root.find_child_mut("rdf", "RDF")?;
            return rdf.find_child_mut("rdf", "Description");
        }
        // rdf:RDF > rdf:Description (first one is ours to modify).
        if root.local == "RDF" && root.prefix == "rdf" {
            return root.find_child_mut("rdf", "Description");
        }
        // Packet without rdf:RDF wrapper: root element itself is a description.
        if root.local == "Description" {
            return Some(root);
        }
        None
    }

    fn description(&self) -> Option<&Element> {
        let root = self.root.as_ref()?;
        if root.local == "xmpmeta" {
            let rdf = root.find_child("rdf", "RDF")?;
            return rdf.find_child("rdf", "Description");
        }
        if root.local == "RDF" && root.prefix == "rdf" {
            return root.find_child("rdf", "Description");
        }
        if root.local == "Description" {
            return Some(root);
        }
        None
    }

    fn ns_declared(&self, prefix: &str) -> bool {
        let key = format!("xmlns:{prefix}");
        let has = |e: &Element| e.attributes.iter().any(|(k, _)| *k == key);
        let Some(root) = self.root.as_ref() else { return false };
        if has(root) {
            return true;
        }
        if root.local == "xmpmeta" && root.find_child("rdf", "RDF").is_some_and(has) {
            return true;
        }
        self.description().is_some_and(has)
    }

    /// The description to write into, with `prefix`'s namespace declared
    /// (on the description) unless an ancestor or the description already
    /// declares it. `<xmp:Rating>` without `xmlns:xmp` is invalid XMP and
    /// other tools would ignore it.
    fn ensure_description_ns(&mut self, prefix: &str) -> &mut Element {
        let needs_ns = !self.ns_declared(prefix);
        let uri = match prefix {
            "xmp" => Some("http://ns.adobe.com/xap/1.0/"),
            "dc" => Some("http://purl.org/dc/elements/1.1/"),
            "lr" => Some("http://ns.adobe.com/lightroom/1.0/"),
            "exif" => Some("http://ns.adobe.com/exif/1.0/"),
            "fb" => Some(FB_NS),
            _ => None,
        };
        let d = self.ensure_description();
        if let (true, Some(uri)) = (needs_ns, uri) {
            d.attributes.push((format!("xmlns:{prefix}"), uri.to_string()));
        }
        d
    }

    fn ensure_description(&mut self) -> &mut Element {
        if self.description_mut().is_none() {
            let desc = Element {
                prefix: "rdf".to_string(),
                local: "Description".to_string(),
                attributes: vec![("rdf:about".to_string(), String::new())],
                children: Vec::new(),
            };
            let root = self.root.get_or_insert_with(|| Element {
                prefix: "rdf".to_string(),
                local: "RDF".to_string(),
                attributes: vec![(
                    "xmlns:rdf".to_string(),
                    "http://www.w3.org/1999/02/22-rdf-syntax-ns#".to_string(),
                )],
                children: Vec::new(),
            });
            root.children.push(Child::Element(desc));
        }
        self.description_mut().expect("just created")
    }
}

/// Append text to an element, merging with a preceding text run (entity
/// references split one value into several parser events). A lone leading
/// whitespace child is absorbed so it is not lost from the value.
fn push_text(parent: &mut Element, text: &str) {
    match parent.children.last_mut() {
        Some(Child::Text(prev)) => prev.push_str(text),
        Some(Child::Whitespace(ws)) => {
            let merged = format!("{ws}{text}");
            parent.children.pop();
            parent.children.push(Child::Text(merged));
        }
        _ => parent.children.push(Child::Text(text.to_string())),
    }
}

fn split_name(name: &str) -> (String, String) {
    match name.split_once(':') {
        Some((p, l)) => (p.to_string(), l.to_string()),
        None => (String::new(), name.to_string()),
    }
}

fn indent(out: &mut Vec<u8>, depth: usize) {
    for _ in 0..depth {
        out.extend_from_slice(b" ");
    }
}

fn write_element(out: &mut Vec<u8>, el: &Element, depth: usize) {
    indent(out, depth);
    out.push(b'<');
    out.extend_from_slice(el.name().as_bytes());
    for (k, v) in &el.attributes {
        out.push(b' ');
        out.extend_from_slice(k.as_bytes());
        out.extend_from_slice(b"=\"");
        out.extend_from_slice(escape_attr(v).as_bytes());
        out.push(b'"');
    }
    if el.children.is_empty() {
        out.extend_from_slice(b"/>\n");
        return;
    }
    out.push(b'>');
    // Emit whitespace children verbatim for byte-stability; text inline.
    let simple_text_only =
        el.children.iter().all(|c| matches!(c, Child::Text(_) | Child::Whitespace(_)));
    if simple_text_only {
        for c in &el.children {
            match c {
                Child::Text(t) => out.extend_from_slice(escape_text(t).as_bytes()),
                Child::Whitespace(t) => out.extend_from_slice(t.as_bytes()),
                _ => unreachable!(),
            }
        }
        out.extend_from_slice(b"</");
        out.extend_from_slice(el.name().as_bytes());
        out.extend_from_slice(b">\n");
    } else {
        out.push(b'\n');
        for c in &el.children {
            match c {
                Child::Element(child) => write_element(out, child, depth + 1),
                Child::Text(t) => {
                    indent(out, depth + 1);
                    out.extend_from_slice(escape_text(t).as_bytes());
                    out.push(b'\n');
                }
                Child::Whitespace(t) => out.extend_from_slice(t.as_bytes()),
                Child::Comment(c) => {
                    indent(out, depth + 1);
                    out.extend_from_slice(b"<!--");
                    out.extend_from_slice(c.as_bytes());
                    out.extend_from_slice(b"-->\n");
                }
                Child::Pi(p) => {
                    indent(out, depth + 1);
                    out.extend_from_slice(b"<?");
                    out.extend_from_slice(p.as_bytes());
                    out.extend_from_slice(b"?>\n");
                }
            }
        }
        indent(out, depth);
        out.extend_from_slice(b"</");
        out.extend_from_slice(el.name().as_bytes());
        out.extend_from_slice(b">\n");
    }
}

fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(ch),
        }
    }
    out
}

fn escape_attr(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Typed view over one XMP sidecar.
#[derive(Debug, Clone)]
pub struct Sidecar {
    tree: XmpTree,
    raw: String,
    from_disk: bool,
}

impl Sidecar {
    /// Read a sidecar. A missing file yields an empty sidecar (created on
    /// first write); a malformed file yields an error, never a panic.
    pub fn read(path: &Path) -> Result<Self, SidecarError> {
        match fs::read(path) {
            Ok(bytes) => {
                let raw = String::from_utf8_lossy(&bytes).into_owned();
                let tree = XmpTree::parse(bytes.as_slice(), path)?;
                Ok(Self { tree, raw, from_disk: true })
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(Self { tree: XmpTree::default(), raw: String::new(), from_disk: false })
            }
            Err(err) => Err(SidecarError::Io { path: path.to_path_buf(), source: err }),
        }
    }

    /// Raw on-disk content the tree was parsed from (for round-trip checks).
    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn existed_on_disk(&self) -> bool {
        self.from_disk
    }

    // ---- typed accessors -----------------------------------------------

    pub fn rating(&self) -> Option<u8> {
        self.tree
            .description()
            .and_then(|d| d.child_text_or_attr("xmp", "Rating"))
            .and_then(|t| t.trim().parse().ok())
    }

    pub fn set_rating(&mut self, rating: u8) {
        let d = self.tree.ensure_description_ns("xmp");
        // An attribute-form `xmp:Rating` would duplicate the child node.
        let attr_name = "xmp:Rating";
        d.attributes.retain(|(k, _)| k != attr_name);
        d.set_or_replace_child("xmp", "Rating", &rating.to_string());
    }

    /// Remove `xmp:Rating` entirely (unstar). A sidecar without a description
    /// is left untouched.
    pub fn clear_rating(&mut self) {
        if let Some(d) = self.tree.description_mut() {
            d.remove_prop("xmp", "Rating");
        }
    }

    pub fn caption(&self) -> Option<String> {
        self.tree.description().and_then(|d| d.child_alt_text("dc", "description"))
    }

    /// Write the caption as the `x-default` entry of `dc:description`.
    pub fn set_caption(&mut self, caption: &str) {
        self.tree.ensure_description_ns("dc").set_alt_default("dc", "description", caption);
    }

    /// Remove `dc:description` entirely (every language: the field is
    /// cleared, nothing resurfaces). A sidecar without a description is
    /// left untouched.
    pub fn clear_caption(&mut self) {
        if let Some(d) = self.tree.description_mut() {
            d.remove_prop("dc", "description");
        }
    }

    pub fn tags(&self) -> Vec<String> {
        self.tree.description().map(|d| d.bag_items("dc", "subject")).unwrap_or_default()
    }

    pub fn set_tags(&mut self, tags: Vec<String>) {
        self.tree.ensure_description_ns("dc").set_or_replace_bag("dc", "subject", &tags);
    }

    /// Lightroom-style hierarchical tags (`lr:hierarchicalSubject`), e.g.
    /// `Vacation|2024`. digiKam and darktable read/write these; PLAN §5.
    pub fn hierarchical_tags(&self) -> Vec<String> {
        self.tree
            .description()
            .map(|d| d.bag_items("lr", "hierarchicalSubject"))
            .unwrap_or_default()
    }

    /// Replace the `lr:hierarchicalSubject` bag (TASK-03-006). Nested tags
    /// use `|` as the path separator, the form darktable and digiKam write.
    pub fn set_hierarchical_tags(&mut self, tags: Vec<String>) {
        self.tree.ensure_description_ns("lr").set_or_replace_bag(
            "lr",
            "hierarchicalSubject",
            &tags,
        );
    }

    /// GPS as (latitude, longitude) decimal degrees.
    pub fn gps(&self) -> Option<(f64, f64)> {
        let d = self.tree.description()?;
        let lat = d.child_text_or_attr("exif", "GPSLatitude")?;
        let lon = d.child_text_or_attr("exif", "GPSLongitude")?;
        Some((parse_gps(&lat), parse_gps(&lon)))
    }

    pub fn set_gps(&mut self, lat: f64, lon: f64) {
        let d = self.tree.ensure_description_ns("exif");
        d.set_or_replace_child("exif", "GPSLatitude", &format_gps(lat, 'N', 'S'));
        d.set_or_replace_child("exif", "GPSLongitude", &format_gps(lon, 'E', 'W'));
    }

    // ---- write path (merge-on-write, 00-010 §3) --------------------------

    /// Write back with merge-on-write discipline: backup previous version
    /// (non-fatal on failure), then atomic replace via `.tmp` + rename.
    pub fn write(&self, path: &Path) -> Result<(), SidecarError> {
        let bak = backup_path(path);

        // Step 4: backup previous version (only if one exists on disk).
        if path.exists() {
            if let Err(err) = fs::copy(path, &bak) {
                tracing::warn!(
                    sidecar = %path.display(),
                    error = %err,
                    "sidecar backup failed; continuing (non-fatal per 00-010 §3.4)"
                );
            }
        }

        // Step 5: atomic replace — write tmp, fsync, rename.
        let tmp = tmp_path(path);
        let bytes = self.tree.serialize();
        {
            let mut f = fs::File::create(&tmp)
                .map_err(|err| SidecarError::Io { path: tmp.clone(), source: err })?;
            f.write_all(&bytes)
                .map_err(|err| SidecarError::Io { path: tmp.clone(), source: err })?;
            f.sync_all().map_err(|err| SidecarError::Io { path: tmp.clone(), source: err })?;
        }
        if let Err(err) = fs::rename(&tmp, path) {
            // Best-effort cleanup so a failed write leaves no stray .tmp.
            let _ = fs::remove_file(&tmp);
            return Err(SidecarError::Io { path: path.to_path_buf(), source: err });
        }
        Ok(())
    }
}

pub fn backup_path(sidecar: &Path) -> PathBuf {
    let mut name = sidecar.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".fb-bak");
    sidecar.with_file_name(name)
}

pub fn tmp_path(sidecar: &Path) -> PathBuf {
    let mut name = sidecar.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(".tmp");
    sidecar.with_file_name(name)
}

/// Parse XMP GPS coordinate text. Handles the forms the interop tools emit:
/// decimal (`52.5331`), DMS (`52°31'59.9"N`), and darktable's
/// deg,decimal-min form (`52,30.9999990N`) — see TASK-03-004.
fn parse_gps(text: &str) -> f64 {
    let t = text.trim();
    if let Ok(v) = t.parse::<f64>() {
        return v;
    }
    // Hemisphere suffix, if any.
    let (body, hemi) = match t.chars().last() {
        Some(h @ ('N' | 'S' | 'E' | 'W')) => (&t[..t.len() - 1], h),
        _ => (t, 'N'),
    };
    // darktable writes `52,30.9999990N` (deg,decimal-minutes with a comma
    // separator); exiv2/digiKam write `52,31N` or `52°31'59.9"N`.
    if body.contains(',') {
        let parts: Vec<f64> = body.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        let deg = match parts.as_slice() {
            [d] => *d,
            [d, m] => d + m / 60.0,
            [d, m, s] => d + m / 60.0 + s / 3600.0,
            _ => return f64::NAN,
        };
        return if hemi == 'S' || hemi == 'W' { -deg } else { deg };
    }
    let body = body.trim_end_matches(|c: char| !c.is_ascii_digit() && c != '.');
    let parts: Vec<f64> = body
        .split(['°', '\'', '"'])
        .filter(|s| !s.trim().is_empty())
        .filter_map(|s| s.trim().parse().ok())
        .collect();
    let mut deg = match parts.as_slice() {
        [d] => *d,
        [d, m] => d + m / 60.0,
        [d, m, s] => d + m / 60.0 + s / 3600.0,
        _ => return f64::NAN,
    };
    if hemi == 'S' || hemi == 'W' {
        deg = -deg;
    }
    deg
}

/// Format decimal degrees as XMP DMS text with hemisphere.
fn format_gps(value: f64, pos: char, neg: char) -> String {
    let hemi = if value >= 0.0 { pos } else { neg };
    let v = value.abs();
    let deg = v.floor();
    let min_full = (v - deg) * 60.0;
    let min = min_full.floor();
    let sec = (min_full - min) * 60.0;
    format!("{deg}\u{b0}{min}'{sec:.1}\"{hemi}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gps_parse_and_format_round_trip() {
        let text = format_gps(52.52, 'N', 'S');
        let parsed = parse_gps(&text);
        assert!((parsed - 52.52).abs() < 1e-3, "{text} -> {parsed}");
        assert!(parse_gps("13.405").abs() - 13.405 < 1e-9);
        assert!((parse_gps("52\u{b0}31'59.9\"N") - 52.533).abs() < 1e-3);
        assert!((parse_gps("52\u{b0}31'59.9\"S") + 52.533).abs() < 1e-3);
        // Real-tool forms (interop fixtures, TASK-03-004): darktable writes
        // deg,decimal-minutes with a comma; exiv2/digiKam deg,integer-min.
        assert!((parse_gps("52,30.9999990N") - (52.0 + 30.999999 / 60.0)).abs() < 1e-3);
        assert!((parse_gps("13,24E") - (13.0 + 24.0 / 60.0)).abs() < 1e-9);
    }

    #[test]
    fn backup_and_tmp_paths() {
        let p = Path::new("/photos/IMG_1.jpg.xmp");
        assert_eq!(backup_path(p), Path::new("/photos/IMG_1.jpg.xmp.fb-bak"));
        assert_eq!(tmp_path(p), Path::new("/photos/IMG_1.jpg.xmp.tmp"));
    }

    #[test]
    fn parse_preserves_comments_and_unknown_attributes() {
        let src = b"<?xml version=\"1.0\"?><a x=\"1\"><!-- keep --><b q=\"z\">v</b></a>";
        let tree = XmpTree::parse(src, Path::new("t")).unwrap();
        let out = String::from_utf8(tree.serialize()).unwrap();
        assert!(out.contains("<!-- keep -->"));
        assert!(out.contains("q=\"z\""));
        assert!(out.contains("x=\"1\""));
    }
}
