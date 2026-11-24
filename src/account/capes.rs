//! Capes: the ones a Minecraft account owns and which of them it wears (the Minecraft services'
//! profile).
//!
//! A Minecraft cape is given by Mojang and listed on the profile, `ACTIVE` or `INACTIVE`; the
//! account wears one (`PUT /minecraft/profile/capes/active` with its id) or none (`DELETE` on the
//! same path), and every client and server shows that one, since it travels in the signed profile.
use super::{bearer, json, request_to, send, str_field, Body, Endpoints, Value};
use crate::i18n::{tr, trf};
use std::io::Read;
use std::time::Duration;

/// Mojang's texture server: the only place a skin's or cape's picture is fetched from.
const TEXTURES: &str = sunrise_launcher_addresses::MINECRAFT_TEXTURE_PATH;
/// The most a picture may weigh: real ones are a few kilobytes.
const MAX_PICTURE: u64 = 256 * 1024;

/// One cape the account owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cape {
    pub id: String,
    /// Its name ("Migrator").
    pub alias: String,
    /// Its picture on textures.minecraft.net.
    pub url: String,
    /// The account wears it.
    pub active: bool,
}

/// What a profile (`/minecraft/profile`) says the account wears: one reading, so the skin and
/// the capes always agree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Profile {
    /// The profile's id (32 hex digits): the account it describes.
    pub id: String,
    /// The skin worn: its URL and model ("classic" or "slim"); none for a default skin.
    pub skin: Option<(String, String)>,
    pub capes: Vec<Cape>,
}

impl Profile {
    /// A profile from the services' answer, every field it is used for checked. None for
    /// anything that isn't wholly one: an empty body, an error page, a `skins` or `capes` field
    /// that isn't a list, an entry without an id or a known state, a picture elsewhere than
    /// Mojang's texture server, an unknown arm model, or more than one skin or cape worn. Such
    /// an answer says nothing about what the account wears (not "no skin, no capes"), so it
    /// never becomes what the page compares edits with.
    pub fn of(v: &Value) -> Option<Profile> {
        let id = str_field(v, "id")?;
        if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        let mut skin = None;
        for entry in v.get("skins")?.as_array()? {
            if !active(entry)? {
                continue;
            }
            let url = str_field(entry, "url").filter(|u| is_texture(u))?;
            let model = match entry.get("variant").and_then(Value::as_str) {
                Some(m) if m.eq_ignore_ascii_case("classic") => "classic",
                Some(m) if m.eq_ignore_ascii_case("slim") => "slim",
                _ => return None,
            };
            if skin.replace((url, model.to_owned())).is_some() {
                return None;
            }
        }
        let capes = v
            .get("capes")?
            .as_array()?
            .iter()
            .map(|c| {
                Some(Cape {
                    id: str_field(c, "id").filter(|id| !id.is_empty())?,
                    alias: match c.get("alias") {
                        None => String::new(),
                        Some(alias) => alias.as_str()?.to_owned(),
                    },
                    url: str_field(c, "url").filter(|u| is_texture(u))?,
                    active: active(c)?,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        if capes.iter().filter(|c| c.active).count() > 1 {
            return None;
        }
        Some(Profile { id, skin, capes })
    }

    /// The profile is the account `uuid`'s (dashes and case aside).
    pub fn is_for(&self, uuid: &str) -> bool {
        self.id.eq_ignore_ascii_case(&uuid.replace('-', ""))
    }

    /// The cape worn, if one is.
    pub fn worn_cape(&self) -> Option<&Cape> {
        self.capes.iter().find(|c| c.active)
    }
}

/// An entry's state: worn (`ACTIVE`) or not (`INACTIVE`); none for anything else.
fn active(entry: &Value) -> Option<bool> {
    match entry.get("state")?.as_str()? {
        "ACTIVE" => Some(true),
        "INACTIVE" => Some(false),
        _ => None,
    }
}

/// What a change the services accepted is known to have left: the profile they sent back or
/// that was read back after it; or only that it was accepted, when no profile could be had
/// after it (and why not). An accepted change is not sent again, nor taken as confirmed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Written {
    Profile(Profile),
    Unconfirmed(String),
}

/// A picture on Mojang's texture server. Profiles list them as plain `http://` addresses; they
/// are only ever fetched over HTTPS ([`https`]).
pub fn is_texture(url: &str) -> bool {
    texture_id(url).is_some()
}

fn texture_id(url: &str) -> Option<&str> {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    rest.strip_prefix(TEXTURES)
        .filter(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric()))
}

/// The HTTPS address of a texture: the one a picture is fetched from.
pub fn https(url: &str) -> Option<String> {
    // Development builds only: `SUNRISE_MINECRAFT_TEXTURES_ENDPOINT` (an address ending in `/`)
    // serves the pictures from a local mock, as `SUNRISE_MINECRAFT_SERVICES_ENDPOINT` does the
    // profile, to see and screenshot the Skins tab.
    let endpoint = std::env::var("SUNRISE_MINECRAFT_TEXTURES_ENDPOINT")
        .ok()
        .filter(|_| cfg!(debug_assertions));
    texture_url(url, endpoint.as_deref())
}

/// The address a texture is fetched from: Mojang's host, or `endpoint` where a development build
/// points it at a local mock. Taking the override as a parameter keeps the decision out of the
/// process environment, which tests share.
fn texture_url(url: &str, endpoint: Option<&str>) -> Option<String> {
    let id = texture_id(url)?;
    Some(match endpoint {
        Some(base) => format!("{base}{id}"),
        None => format!("https://{TEXTURES}{id}"),
    })
}

/// What the account with `token` wears now.
pub fn read(ep: &Endpoints, token: &str) -> Result<Profile, String> {
    let (status, v) = send(
        bearer(
            request_to("GET", &format!("{}/minecraft/profile", ep.minecraft))?,
            token,
        ),
        None,
        "the Minecraft services",
    )?;
    match status {
        200 => Profile::of(&v).ok_or_else(|| unreadable().into()),
        401 => Err(tr(super::SIGN_IN_AGAIN).into()),
        429 => Err(tr("The Minecraft services are busy. Wait a minute, then try again.").into()),
        _ => Err(trf(
            "The Minecraft services couldn't say what this account wears (error {status}). Try again later.",
            &[("status", &status.to_string())],
        )),
    }
}

fn unreadable() -> &'static str {
    tr("The Minecraft services sent an answer the launcher can't read. Try again later.")
}

/// What a change the services accepted left: the profile they sent back, else read again,
/// since an acknowledgement without a usable profile confirms nothing about what is worn. A
/// read-back that fails leaves the change accepted but unconfirmed, never refused.
pub(super) fn after_change(ep: &Endpoints, token: &str, v: &Value) -> Written {
    match Profile::of(v) {
        Some(profile) => Written::Profile(profile),
        None => match read(ep, token) {
            Ok(profile) => Written::Profile(profile),
            Err(why) => Written::Unconfirmed(why),
        },
    }
}

/// Wears the cape `id`, or none: what the change left, or why the services refused it.
pub fn wear(ep: &Endpoints, token: &str, id: Option<&str>) -> Result<Written, String> {
    let url = format!("{}/minecraft/profile/capes/active", ep.minecraft);
    let (status, v) = match id {
        Some(id) => send(
            bearer(request_to("PUT", &url)?, token),
            Some(Body::Json(json!({ "capeId": id }))),
            "the Minecraft services",
        )?,
        None => send(
            bearer(request_to("DELETE", &url)?, token),
            None,
            "the Minecraft services",
        )?,
    };
    match status {
        200 | 204 => Ok(after_change(ep, token, &v)),
        401 => Err(tr(super::SIGN_IN_AGAIN).into()),
        400 | 404 => Err(tr("This account doesn't own that cape.").into()),
        429 => {
            Err(tr("Too many cape changes in a short time. Wait a minute, then try again.").into())
        }
        _ => Err(trf(
            "The Minecraft services couldn't change the cape (error {status}). Try again later.",
            &[("status", &status.to_string())],
        )),
    }
}

/// A skin's or cape's picture from Mojang's texture server, over HTTPS and never redirected
/// elsewhere; none when it can't be had.
pub fn picture(url: &str) -> Option<Vec<u8>> {
    picture_from(&https(url)?)
}

fn picture_from(address: &str) -> Option<Vec<u8>> {
    let agent = crate::proxy::agent("textures", address, |builder| {
        builder.timeout(Duration::from_secs(10)).redirects(0)
    })
    .ok()?;
    let response = agent.get(address).call().ok()?;
    if response.status() != 200 {
        return None;
    }
    read_picture(response.into_reader())
}

/// At most [`MAX_PICTURE`] bytes that decode as a PNG of a sane size.
fn read_picture(reader: impl Read) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(MAX_PICTURE + 1).read_to_end(&mut bytes).ok()?;
    let fits = bytes.len() as u64 <= MAX_PICTURE;
    (fits && crate::player_model::decode(&bytes, 2048, 1024).is_some()).then_some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "069a79f444e94726a5befca90e38aaf5";

    #[test]
    fn textures_are_fetched_over_https_only_from_mojang() {
        assert_eq!(
            texture_url("http://textures.minecraft.net/texture/abc123", None).as_deref(),
            Some("https://textures.minecraft.net/texture/abc123")
        );
        for elsewhere in [
            "http://textures.minecraft.net.example.com/texture/abc",
            "http://example.com/texture/abc",
            "http://textures.minecraft.net/texture/../x",
            "http://textures.minecraft.net/texture/",
            "ftp://textures.minecraft.net/texture/abc",
        ] {
            assert_eq!(texture_url(elsewhere, None), None, "{elsewhere}");
        }
    }

    #[test]
    fn development_texture_endpoint_overrides_the_built_in_host() {
        assert_eq!(
            texture_url(
                "http://textures.minecraft.net/texture/abc",
                Some("http://127.0.0.1:9/tex/")
            )
            .as_deref(),
            Some("http://127.0.0.1:9/tex/abc")
        );
        assert_eq!(
            texture_url("http://textures.minecraft.net/texture/abc", None).as_deref(),
            Some("https://textures.minecraft.net/texture/abc")
        );
    }

    #[test]
    fn a_body_that_is_not_a_profile_is_not_an_empty_one() {
        assert_eq!(Profile::of(&Value::Null), None);
        assert_eq!(Profile::of(&json!({})), None);
        assert_eq!(Profile::of(&json!({"capes": []})), None, "no id, no skins");
        assert_eq!(
            Profile::of(&json!({"id": ID, "skins": []})),
            None,
            "no capes field is not an empty list of them"
        );
        let profile = Profile::of(&json!({"id": ID, "skins": [], "capes": []})).unwrap();
        assert_eq!(profile.skin, None);
        assert!(profile.capes.is_empty());
        assert!(profile.is_for("069a79f4-44e9-4726-a5be-fca90e38aaf5"));
        assert!(!profile.is_for("b09600dff4354d4fb56776eec1e55083"));
    }

    #[test]
    fn a_profile_with_a_malformed_field_is_no_profile() {
        let skin = json!({"id": "s", "state": "ACTIVE", "variant": "SLIM",
                          "url": "http://textures.minecraft.net/texture/abc"});
        let cape = json!({"id": "c1", "state": "ACTIVE", "alias": "Migrator",
                          "url": "http://textures.minecraft.net/texture/c1"});
        let good = json!({"id": ID, "skins": [skin], "capes": [cape]});
        let read = Profile::of(&good).unwrap();
        assert_eq!(
            read.skin,
            Some((
                "http://textures.minecraft.net/texture/abc".into(),
                "slim".into()
            ))
        );
        assert_eq!(read.worn_cape().map(|c| c.id.as_str()), Some("c1"));
        let with = |pointer: &str, value: Value| {
            let mut v = good.clone();
            *v.pointer_mut(pointer).unwrap() = value;
            v
        };
        for (why, broken) in [
            ("capes not a list", with("/capes", json!({"c1": true}))),
            ("capes null", with("/capes", Value::Null)),
            ("skins not a list", with("/skins", json!("abc"))),
            ("an id that isn't one", with("/id", json!("Notch"))),
            ("a cape that isn't an object", with("/capes/0", json!("c1"))),
            ("a cape without an id", with("/capes/0/id", Value::Null)),
            (
                "a cape without a state",
                with("/capes/0/state", Value::Null),
            ),
            (
                "a cape in an unknown state",
                with("/capes/0/state", json!("WORN")),
            ),
            (
                "a cape pictured elsewhere",
                with("/capes/0/url", json!("http://example.com/texture/c1")),
            ),
            (
                "a cape alias that isn't text",
                with("/capes/0/alias", json!(7)),
            ),
            (
                "a skin in an unknown state",
                with("/skins/0/state", json!(1)),
            ),
            (
                "a skin with no arm model",
                with("/skins/0/variant", Value::Null),
            ),
            (
                "an unknown arm model",
                with("/skins/0/variant", json!("WIDE")),
            ),
            (
                "a skin pictured elsewhere",
                with("/skins/0/url", json!("http://example.com/texture/abc")),
            ),
            (
                "two capes worn",
                with(
                    "/capes",
                    json!([good["capes"][0], {"id": "c2", "state": "ACTIVE",
                    "url": "http://textures.minecraft.net/texture/c2"}]),
                ),
            ),
            (
                "two skins worn",
                with("/skins", json!([good["skins"][0], good["skins"][0]])),
            ),
        ] {
            assert_eq!(Profile::of(&broken), None, "{why}");
        }
        // An inactive skin needs no picture: only the worn one is ever shown.
        let old = with(
            "/skins",
            json!([good["skins"][0], {"id": "old", "state": "INACTIVE"}]),
        );
        assert_eq!(Profile::of(&old).unwrap().skin, read.skin);
    }
}
