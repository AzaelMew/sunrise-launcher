//! Pinned native Markdown and its optional images: files of the same repository at the pinned
//! commit, or GitHub's own README attachments. Reading a README never loads an image: each typed
//! image is fetched only when its rendering code needs it.
use super::{
    bounded, digest,
    github::{Failure, Github, STATE},
    safe_path,
};
use crate::i18n::tr;
use image::{ImageDecoder, ImageFormat};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Cursor,
    path::{Path, PathBuf},
};
use url::Url;

pub(super) const MAX_README: u64 = 128 << 10;
const MAX_IMAGES: usize = 8;
const MAX_IMAGE: u64 = 2 << 20;
const MAX_EDGE: u32 = 2048;

#[derive(Clone)]
pub struct Readme {
    markdown: String,
    base: Url,
    images: Vec<ReadmeImage>,
}

impl Readme {
    pub fn markdown(&self) -> &str {
        &self.markdown
    }
    pub fn images(&self) -> &[ReadmeImage] {
        &self.images
    }

    /// Links open only on an explicit click. They are never fetched by discovery or rendering.
    pub fn link(&self, destination: &str) -> Option<String> {
        if destination.contains('\\')
            || destination.contains(char::is_control)
            || destination.starts_with("//")
        {
            return None;
        }
        let url = self.base.join(destination).ok()?;
        (url.scheme() == "https"
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none())
        .then(|| url.into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ReadmeImage {
    destination: String,
    alt: String,
    repo: String,
    sha: String,
    location: Location,
    allow_fetch: bool,
}

/// Where an image's bytes come from.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Location {
    /// A file of the listed repository at the pinned commit.
    Repo(String),
    /// `https://github.com/user-attachments/assets/<uuid>`: an image uploaded to GitHub for the
    /// README, which answers with one redirect to GitHub's asset host.
    Attachment(String),
}

impl ReadmeImage {
    pub fn destination(&self) -> &str {
        &self.destination
    }
    pub fn alt(&self) -> &str {
        &self.alt
    }
}

pub(super) fn markdown_file(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|extension| {
            ["md", "markdown", "mdown", "mkd"]
                .iter()
                .any(|kind| extension.eq_ignore_ascii_case(kind))
        })
}

const ATTACHMENTS: &str = "https://github.com/user-attachments/assets/";

fn image_location(repo: &str, sha: &str, readme_path: &str, dest: &str) -> Option<Location> {
    if let Some(uuid) = dest.strip_prefix(ATTACHMENTS) {
        return attachment_id(uuid).then(|| Location::Attachment(uuid.to_owned()));
    }
    image_path(repo, sha, readme_path, dest).map(Location::Repo)
}

/// A GitHub attachment id: a UUID, with nothing after it.
fn attachment_id(uuid: &str) -> bool {
    let groups: Vec<_> = uuid.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(group, len)| group.len() == len && group.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// The one redirect an attachment may take: plain HTTPS to GitHub's own asset hosts.
pub(super) fn asset_redirect(location: &str) -> bool {
    let Ok(url) = Url::parse(location) else {
        return false;
    };
    let host = url.host_str().unwrap_or_default();
    url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && (host.ends_with(".githubusercontent.com")
            || host
                .strip_prefix("github-production-user-asset-")
                .and_then(|rest| rest.strip_suffix(".s3.amazonaws.com"))
                .is_some_and(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric())))
}

fn image_path(repo: &str, sha: &str, readme_path: &str, dest: &str) -> Option<String> {
    let dest = dest.strip_prefix("./").unwrap_or(dest);
    let raw = format!("https://raw.githubusercontent.com/{repo}/{sha}/");
    let blob = format!("https://github.com/{repo}/blob/{sha}/");
    let path = if let Some(relative) = dest.strip_prefix(&raw).or_else(|| dest.strip_prefix(&blob))
    {
        relative.to_owned()
    } else if let Some(root) = dest.strip_prefix('/') {
        root.to_owned()
    } else if let Some((dir, _)) = readme_path.rsplit_once('/') {
        format!("{dir}/{dest}")
    } else {
        dest.to_owned()
    };
    // Validation precedes URL normalization: no percent escapes, protocol-relative URLs,
    // traversal, credentials, or a repository/ref from the Markdown can reach the network.
    safe_path(&path).then_some(path)
}

#[derive(Serialize, Deserialize)]
struct Saved {
    path: String,
    markdown: String,
}

impl Saved {
    fn parse(self, repo: &str, sha: &str, allow_fetch: bool) -> Readme {
        let mut base = Url::parse("https://github.com/").expect("constant URL");
        base.path_segments_mut()
            .expect("HTTPS URL")
            .pop_if_empty()
            .extend(repo.split('/'))
            .push("blob")
            .push(sha)
            .extend(self.path.split('/'));
        let markdown = strip_html(self.markdown);
        let mut images = Vec::new();
        let mut looking = None;
        for event in parser(&markdown) {
            match event {
                Event::Start(Tag::Image { dest_url, .. }) if looking.is_none() => {
                    looking = Some((dest_url.into_string(), String::new()));
                }
                Event::Text(text) | Event::Code(text) if looking.is_some() => {
                    looking.as_mut().expect("inside image").1.push_str(&text);
                }
                Event::End(TagEnd::Image) => {
                    if let Some((destination, alt)) = looking.take() {
                        if let Some(location) = image_location(repo, sha, &self.path, &destination)
                        {
                            images.push(ReadmeImage {
                                destination,
                                alt,
                                repo: repo.to_owned(),
                                sha: sha.to_owned(),
                                location,
                                allow_fetch,
                            });
                            if images.len() == MAX_IMAGES {
                                break;
                            }
                        }
                    }
                }
                _ => (),
            }
        }
        Readme {
            markdown,
            base,
            images,
        }
    }
}

fn parser(markdown: &str) -> Parser<'_> {
    Parser::new_ext(
        markdown,
        Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TABLES,
    )
}

/// Remove all HTML. An `<img>` element's `src` and `alt` stay, as a Markdown image, so it meets
/// the same image rules as one written in Markdown.
fn strip_html(markdown: String) -> String {
    let mut clean = String::with_capacity(markdown.len());
    let mut next = 0;
    // The parser hands over block HTML a line at a time, and inline HTML a tag at a time: read
    // each HTML block, and each paragraph's inline HTML, as one fragment.
    let (mut block, mut inline) = (String::new(), String::new());
    // A comment or text-only element left open in one fragment hides what follows it until it
    // closes, across Markdown boundaries.
    let mut open = None;
    let mut in_block = false;
    for (event, span) in parser(&markdown).into_offset_iter() {
        let html = match &event {
            Event::Start(Tag::HtmlBlock) => {
                push_images(&mut clean, &mut inline, false, &mut open);
                in_block = true;
                continue;
            }
            Event::End(TagEnd::HtmlBlock) => {
                in_block = false;
                push_images(&mut clean, &mut block, true, &mut open);
                continue;
            }
            Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::TableCell | TagEnd::Item,
            ) => {
                push_images(&mut clean, &mut inline, false, &mut open);
                continue;
            }
            Event::Html(html) | Event::InlineHtml(html) => html,
            _ => continue,
        };
        if next < span.start {
            clean.push_str(&markdown[next..span.start]);
        }
        next = next.max(span.end);
        match (&event, in_block) {
            (Event::InlineHtml(_), _) | (_, false) => inline.push_str(html),
            _ => block.push_str(html),
        }
    }
    push_images(&mut clean, &mut inline, false, &mut open);
    push_images(&mut clean, &mut block, true, &mut open);
    clean.push_str(&markdown[next..]);
    clean
}

/// Write the images of an HTML fragment as Markdown images, and empty the fragment.
fn push_images(clean: &mut String, fragment: &mut String, block: bool, open: &mut Option<String>) {
    for (alt, src) in html_images(fragment, open) {
        clean.push_str("![");
        for c in alt.chars().filter(|c| !c.is_control()) {
            if c.is_ascii_punctuation() {
                clean.push('\\');
            }
            clean.push(c);
        }
        clean.push_str("](<");
        clean.push_str(&src);
        clean.push_str(">)");
        if block {
            clean.push_str("\n\n");
        }
    }
    fragment.clear();
}

/// The `alt` and `src` of each `img` element in an HTML fragment. Comments, `script` and `style`
/// text, and the attributes of other elements are skipped whole. A `src` that could break out of
/// a Markdown destination is dropped.
fn html_images(html: &str, open: &mut Option<String>) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut rest = html;
    loop {
        if let Some(close) = open.as_deref() {
            let Some(at) = closing(rest, close) else {
                return found;
            };
            rest = &rest[at..];
            *open = None;
        }
        let Some(lt) = rest.find('<') else {
            break;
        };
        rest = &rest[lt + 1..];
        if let Some(comment) = rest.strip_prefix("!--") {
            rest = comment;
            *open = Some("-->".into());
            continue;
        }
        // The whole tag name: `img-preview` is not `img`.
        let name_end = rest
            .find(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
            .unwrap_or(rest.len());
        let name = rest[..name_end].to_ascii_lowercase();
        let (attributes, after) = tag_attributes(&rest[name_end..]);
        rest = after;
        match name.as_str() {
            "img" => {
                let value = |key: &str| {
                    attributes
                        .iter()
                        .find(|(name, _)| name == key)
                        .map(|(_, value)| value.clone())
                };
                if let Some(src) = value("src").filter(|src| {
                    !src.is_empty()
                        && !src.contains(|c: char| {
                            c.is_whitespace() || c.is_control() || c == '<' || c == '>'
                        })
                }) {
                    found.push((value("alt").unwrap_or_default(), src));
                }
            }
            "script" | "style" | "textarea" | "title" => *open = Some(format!("</{name}")),
            _ => (),
        }
    }
    found
}

/// Where text hidden by an open comment or text-only element ends: after `-->`, or at the
/// matching end tag (`</script` followed by the name's end). ASCII lowercasing keeps offsets.
fn closing(rest: &str, close: &str) -> Option<usize> {
    if close == "-->" {
        return rest.find(close).map(|at| at + close.len());
    }
    let lower = rest.to_ascii_lowercase();
    lower.match_indices(close).map(|(at, _)| at).find(|&at| {
        lower[at + close.len()..]
            .starts_with(|c: char| c.is_ascii_whitespace() || c == '/' || c == '>')
    })
}

/// A tag's attributes, names lowercased and values decoded, and the text after its closing `>`.
fn tag_attributes(mut rest: &str) -> (Vec<(String, String)>, &str) {
    let mut attributes = Vec::new();
    loop {
        rest = rest.trim_start_matches(|c: char| c.is_ascii_whitespace() || c == '/');
        if let Some(after) = rest.strip_prefix('>') {
            return (attributes, after);
        }
        let name_end = rest
            .find(|c: char| c.is_ascii_whitespace() || matches!(c, '=' | '/' | '>'))
            .unwrap_or(rest.len());
        if name_end == 0 {
            return (attributes, "");
        }
        let name = rest[..name_end].to_ascii_lowercase();
        rest = rest[name_end..].trim_start();
        let mut value = "";
        if let Some(after) = rest.strip_prefix('=') {
            let after = after.trim_start();
            let (text, tail) = match after.chars().next() {
                Some(quote @ ('"' | '\'')) => {
                    let body = &after[1..];
                    let close = body.find(quote).unwrap_or(body.len());
                    (&body[..close], body.get(close + 1..).unwrap_or(""))
                }
                _ => {
                    let close = after
                        .find(|c: char| c.is_ascii_whitespace() || c == '>')
                        .unwrap_or(after.len());
                    (&after[..close], &after[close..])
                }
            };
            value = text;
            rest = tail;
        }
        attributes.push((name, decode_entities(value)));
    }
}

/// HTML's character references: the named ones an attribute needs, and numeric ones.
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let decoded = rest.find(';').and_then(|end| {
            let name = &rest[1..end];
            let c = match name {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                "nbsp" => '\u{a0}',
                _ => {
                    let number = name.strip_prefix('#')?;
                    let code = match number.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => number.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((c, end + 1))
        });
        match decoded {
            Some((c, len)) => {
                out.push(c);
                rest = &rest[len..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub(super) fn load(
    github: &Github,
    repo: &str,
    sha: &str,
    offline: bool,
    read: impl FnOnce() -> Result<Option<(String, Vec<u8>)>, Failure>,
) -> Result<Option<Readme>, Failure> {
    let key = digest(format!("{repo}/{sha}").as_bytes());
    let target = github.cache.join(format!("readme-{key}.json"));
    let saved = fs::File::open(&target).ok().and_then(|file| {
        let bytes = bounded(file, MAX_README * 6 + 4096).ok()?;
        let saved: Saved = serde_json::from_slice(&bytes).ok()?;
        (safe_path(&saved.path)
            && markdown_file(&saved.path)
            && saved.markdown.len() as u64 <= MAX_README)
            .then_some(saved)
    });
    if let Some(saved) = saved {
        return Ok(Some(saved.parse(repo, sha, !offline)));
    }
    if offline {
        return Ok(None);
    }
    let Some((path, bytes)) = read()? else {
        return Ok(None);
    };
    let Ok(markdown) = String::from_utf8(bytes) else {
        return Ok(None);
    };
    let saved = Saved { path, markdown };
    if let Ok(bytes) = serde_json::to_vec(&saved) {
        if let Err(error) = github.save(&target, &bytes) {
            eprintln!("Save pinned README: {error}");
        }
    }
    Ok(Some(saved.parse(repo, sha, true)))
}

/// Validate byte length, dimensions before decode, media type and lack of animation.
fn check_image(bytes: &[u8]) -> Result<&'static str, String> {
    let fail = || tr("README image unavailable.").to_string();
    if bytes.len() as u64 > MAX_IMAGE {
        return Err(fail());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_EDGE);
    limits.max_image_height = Some(MAX_EDGE);
    limits.max_alloc = Some(48 << 20);
    let format = image::guess_format(bytes).map_err(|_| fail())?;
    let suffix = match format {
        ImageFormat::Png => {
            let png =
                image::codecs::png::PngDecoder::with_limits(Cursor::new(bytes), limits.clone())
                    .map_err(|_| fail())?;
            if png.is_apng().map_err(|_| fail())? {
                return Err(fail());
            }
            "png"
        }
        ImageFormat::Jpeg => "jpg",
        ImageFormat::WebP => {
            let decoder =
                image::codecs::webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|_| fail())?;
            let (w, h) = decoder.dimensions();
            if w > MAX_EDGE || h > MAX_EDGE || decoder.has_animation() {
                return Err(fail());
            }
            "webp"
        }
        _ => return Err(fail()),
    };
    let mut reader = image::ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    reader.decode().map_err(|_| fail())?;
    Ok(suffix)
}

pub fn readme_image(image: &ReadmeImage) -> Result<PathBuf, String> {
    let github = Github::current();
    let key = match &image.location {
        Location::Repo(path) => digest(format!("{}/{}/{path}", image.repo, image.sha).as_bytes()),
        Location::Attachment(uuid) => digest(format!("user-attachments/{uuid}").as_bytes()),
    };
    let unavailable = || tr("README image unavailable.").to_string();
    let target = |suffix| github.cache.join(format!("image-{key}.{suffix}"));
    let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
    for suffix in ["png", "jpg", "webp"] {
        let saved = target(suffix);
        if fs::File::open(&saved)
            .ok()
            .and_then(|file| bounded(file, MAX_IMAGE).ok())
            .is_some_and(|bytes| check_image(&bytes).ok() == Some(suffix))
        {
            return Ok(saved);
        }
    }
    if !image.allow_fetch {
        return Err(unavailable());
    }
    let bytes = match &image.location {
        Location::Repo(path) => github.raw(&mut state, &image.repo, &image.sha, path, MAX_IMAGE),
        Location::Attachment(uuid) => github.attachment(&mut state, uuid, MAX_IMAGE),
    }
    .map_err(|_| unavailable())?
    .ok_or_else(unavailable)?;
    let file = target(check_image(&bytes)?);
    github.save(&file, &bytes).map_err(|_| unavailable())?;
    Ok(file)
}

#[cfg(test)]
mod tests;
