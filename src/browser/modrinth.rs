//! Modrinth's public API (api.modrinth.com/v2): no key, files with SHA-512 and SHA-1.
use super::{details, FileRef, Kind, Project, Query, Source, SourceId, Version};
use serde_json::Value;

pub(crate) const API: &str = "https://api.modrinth.com/v2";
const SITE: &str = "https://modrinth.com";

pub struct Modrinth;

/// Modrinth's project type for a kind it is searched for: packs only. Java mods and modpacks
/// are not offered from Modrinth.
fn project_type(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::ResourcePack => Some("resourcepack"),
        Kind::Shader => Some("shader"),
        Kind::DataPack => Some("datapack"),
        Kind::Mod | Kind::Modpack => None,
    }
}

const PACKS_ONLY: &str =
    "Modrinth is searched for resource packs, data packs and shader packs only.";

/// The kind of a Modrinth project; its shader packs are Iris-format ones ([`Project::support`]
/// says which run). A data pack says it is a `mod` (most also ship a mod build): the search for data packs marks
/// its hits, and an install into a world asks for its data pack files. Nothing else is kept as a
/// mod: [`Project::support`] refuses it.
fn kind_of(project_type: &str) -> Option<Kind> {
    match project_type {
        "resourcepack" => Some(Kind::ResourcePack),
        "shader" => Some(Kind::Shader),
        "mod" => Some(Kind::Mod),
        _ => None,
    }
}

/// The search's facets (AND of ORs): the pack type, the categories, and unless `everything` what
/// runs.
pub(super) fn facets(query: &Query) -> Option<String> {
    let f = &query.filters;
    let mut all = vec![vec![format!("project_type:{}", project_type(query.kind)?)]];
    match &f.game_version {
        Some(v) => all.push(vec![format!("versions:{v}")]),
        None if !query.everything => all.push(vec![format!("versions:{}", query.game)]),
        None => {}
    }
    // Shader packs: only the Iris format (`iris`), whatever else a pack also lists.
    if query.kind == Kind::Shader {
        all.push(vec!["categories:iris".into()]);
    }
    for c in &f.categories {
        all.push(vec![format!("categories:{c}")]);
    }
    Some(serde_json::to_string(&all).unwrap_or_default())
}

pub(super) fn search_url(query: &Query) -> Option<String> {
    let mut url = format!(
        "{API}/search?facets={}&index={}&offset={}&limit={}",
        encode(&facets(query)?),
        query.filters.sort(&query.text).modrinth(),
        query.offset,
        query.limit.clamp(1, 100),
    );
    if !query.text.is_empty() {
        url.push_str("&query=");
        url.push_str(&encode(&query.text));
    }
    Some(url)
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_owned))
        .collect()
}

fn text(v: &Value) -> String {
    v.as_str().unwrap_or_default().to_owned()
}

/// A search hit. Its `categories` hold the loaders too; `versions` its Minecraft versions.
pub(super) fn parse_hit(hit: &Value) -> Option<Project> {
    let kind = kind_of(hit["project_type"].as_str()?)?;
    let slug = text(&hit["slug"]);
    Some(Project {
        source: SourceId::Modrinth,
        id: hit["project_id"].as_str()?.to_owned(),
        page: format!("{SITE}/{}/{slug}", project_type(kind).unwrap_or("mod")),
        slug,
        title: text(&hit["title"]),
        summary: text(&hit["description"]),
        author: text(&hit["author"]),
        downloads: hit["downloads"].as_u64().unwrap_or(0),
        icon: hit["icon_url"]
            .as_str()
            .filter(|u| !u.is_empty())
            .map(str::to_owned),
        license: text(&hit["license"]),
        kind,
        game_versions: strings(&hit["versions"]),
        loaders: strings(&hit["categories"]),
        blocked: None,
    })
}

/// `GET /project/{id}`: the fields differ from a search hit's.
pub(super) fn parse_project(p: &Value) -> Option<Project> {
    let kind = kind_of(p["project_type"].as_str()?)?;
    let slug = text(&p["slug"]);
    Some(Project {
        source: SourceId::Modrinth,
        id: p["id"].as_str()?.to_owned(),
        page: format!("{SITE}/{}/{slug}", project_type(kind).unwrap_or("mod")),
        slug,
        title: text(&p["title"]),
        summary: text(&p["description"]),
        author: String::new(),
        downloads: p["downloads"].as_u64().unwrap_or(0),
        icon: p["icon_url"]
            .as_str()
            .filter(|u| !u.is_empty())
            .map(str::to_owned),
        license: text(&p["license"]["id"]),
        kind,
        game_versions: strings(&p["game_versions"]),
        loaders: strings(&p["loaders"]),
        blocked: None,
    })
}

pub(super) fn parse_version(v: &Value) -> Option<Version> {
    let files = v["files"]
        .as_array()?
        .iter()
        .filter_map(|f| {
            Some(FileRef {
                url: f["url"].as_str()?.to_owned(),
                name: f["filename"].as_str()?.to_owned(),
                size: f["size"].as_u64().unwrap_or(0),
                sha512: f["hashes"]["sha512"].as_str().map(str::to_ascii_lowercase),
                sha256: None,
                sha1: f["hashes"]["sha1"].as_str().map(str::to_ascii_lowercase),
                primary: f["primary"].as_bool().unwrap_or(false),
            })
        })
        .collect();
    let required = v["dependencies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| d["dependency_type"] == "required")
        .filter_map(|d| d["project_id"].as_str().map(str::to_owned))
        .collect();
    Some(Version {
        id: v["id"].as_str()?.to_owned(),
        number: text(&v["version_number"]),
        published: text(&v["date_published"]),
        game_versions: strings(&v["game_versions"]),
        loaders: strings(&v["loaders"]),
        files,
        required,
        blocked: None,
    })
}

/// The files of one version (`GET /version/{id}`), with their downloads and checksums.
pub fn version_files(version: &str) -> Result<Vec<FileRef>, String> {
    let v = get(&format!("{API}/version/{}", encode(version)))?;
    parse_version(&v)
        .map(|v| v.files)
        .ok_or_else(|| format!("Modrinth: no version {version}."))
}

/// Projects' icons in one request (`GET /projects?ids=[…]`, at most 100 ids): id → icon URL, for
/// those that have one.
pub fn icons(ids: &[String]) -> Result<Vec<(String, String)>, String> {
    let ids = &ids[..ids.len().min(100)];
    let list = serde_json::to_string(ids).map_err(|e| e.to_string())?;
    let v = get(&format!("{API}/projects?ids={}", encode(&list)))?;
    Ok(v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| Some((text(&p["id"]), p["icon_url"].as_str()?.to_owned())))
        .filter(|(id, url)| !id.is_empty() && !url.is_empty())
        .collect())
}

impl Source for Modrinth {
    fn id(&self) -> SourceId {
        SourceId::Modrinth
    }

    fn search(&self, query: &Query) -> Result<Vec<Project>, String> {
        let found = get(&search_url(query).ok_or(PACKS_ONLY)?)?;
        Ok(found["hits"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(parse_hit)
            .map(|p| match query.kind {
                Kind::DataPack => p.as_datapack(),
                _ => p,
            })
            .collect())
    }

    fn project(&self, id: &str) -> Result<Project, String> {
        let p = get(&format!("{API}/project/{}", encode(id)))?;
        parse_project(&p).ok_or_else(|| format!("Modrinth: {id} is not a pack Sunrise loads."))
    }

    fn versions(&self, project: &Project) -> Result<Vec<Version>, String> {
        let found = get(&format!("{API}/project/{}/version", encode(&project.id)))?;
        let mut all: Vec<Version> = found
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| v["status"].as_str().is_none_or(|s| s == "listed"))
            .filter_map(parse_version)
            .collect();
        // Modrinth lists newest first already; ISO dates sort as text.
        all.sort_by(|a, b| b.published.cmp(&a.published));
        Ok(all)
    }

    fn details(&self, project: &Project) -> Result<details::Details, String> {
        let id = encode(&project.id);
        let p = get(&format!("{API}/project/{id}"))?;
        let members = get(&format!("{API}/project/{id}/members")).unwrap_or_default();
        Ok(details::modrinth_details(&p, &members))
    }
}

fn get(url: &str) -> Result<Value, String> {
    let response = super::download::agent(url)
        .get(url)
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(429, _) => {
                "Modrinth asks to slow down: try again in a minute.".into()
            }
            ureq::Error::Status(code, _) => format!("Modrinth answered {code}."),
            ureq::Error::Transport(t) => format!("No connection to Modrinth: {t}"),
        })?;
    response
        .into_json()
        .map_err(|e| format!("Modrinth's answer wasn't readable: {e}"))
}

/// Percent-encoding for a query value or a path segment.
pub(super) fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
