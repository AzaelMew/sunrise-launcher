//! Capes: the ones a Minecraft account owns and which of them it wears (the Minecraft services'
//! profile).
//!
//! A Minecraft cape is given by Mojang and listed on the profile, `ACTIVE` or `INACTIVE`; the
//! account wears one (`PUT /minecraft/profile/capes/active` with its id) or none (`DELETE` on the
//! same path), and every client and server shows that one, since it travels in the signed profile.
use super::{bearer, json, request_to, send, str_field, Body, Endpoints, Value};
use std::io::Read;
use std::time::Duration;

/// Mojang's texture server: the only place a skin's or cape's picture is fetched from.
const TEXTURES: &str = "textures.minecraft.net/texture/";
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
    /// The skin worn: its URL and model ("classic" or "slim"); none for a default skin.
    pub skin: Option<(String, String)>,
    pub capes: Vec<Cape>,
}

impl Profile {
    /// A profile from the services' answer. None for anything that isn't one (an empty body,
    /// an error page): that says nothing about what the account wears, not "no skin, no capes".
    pub fn of(v: &Value) -> Option<Profile> {
        str_field(v, "id")?;
        v.get("skins")?.as_array()?;
        Some(Profile {
            skin: worn_skin(v),
            capes: owned(v),
        })
    }

    /// The URL of the cape worn, if one is.
    pub fn worn_cape(&self) -> Option<&Cape> {
        self.capes.iter().find(|c| c.active)
    }
}

/// The capes a profile lists, in its order.
pub fn owned(profile: &Value) -> Vec<Cape> {
    profile
        .get("capes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| {
            Some(Cape {
                id: str_field(c, "id")?,
                alias: str_field(c, "alias").unwrap_or_default(),
                url: str_field(c, "url").filter(|u| is_texture(u))?,
                active: c.get("state").and_then(Value::as_str) == Some("ACTIVE"),
            })
        })
        .collect()
}

/// The skin a profile wears: its URL and model.
fn worn_skin(profile: &Value) -> Option<(String, String)> {
    let skin = profile
        .get("skins")?
        .as_array()?
        .iter()
        .find(|s| s.get("state").and_then(Value::as_str) == Some("ACTIVE"))?;
    Some((
        str_field(skin, "url").filter(|u| is_texture(u))?,
        str_field(skin, "variant")
            .unwrap_or_else(|| "CLASSIC".into())
            .to_lowercase(),
    ))
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
    Some(format!("https://{TEXTURES}{}", texture_id(url)?))
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
        401 => Err(super::SIGN_IN_AGAIN.into()),
        429 => Err("The Minecraft services are busy. Wait a minute, then try again.".into()),
        _ => Err(format!(
            "The Minecraft services couldn't say what this account wears (error {status}). Try again later."
        )),
    }
}

fn unreadable() -> &'static str {
    "The Minecraft services sent an answer the launcher can't read. Try again later."
}

/// The profile after a change the services accepted: the one they sent back, else read again,
/// since an acknowledgement without a usable profile confirms nothing about what is worn.
pub(super) fn after_change(ep: &Endpoints, token: &str, v: &Value) -> Result<Profile, String> {
    match Profile::of(v) {
        Some(profile) => Ok(profile),
        None => read(ep, token),
    }
}

/// Wears the cape `id`, or none; the profile as it is after it.
pub fn wear(ep: &Endpoints, token: &str, id: Option<&str>) -> Result<Profile, String> {
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
        200 | 204 => after_change(ep, token, &v),
        401 => Err(super::SIGN_IN_AGAIN.into()),
        400 | 404 => Err("This account doesn't own that cape.".into()),
        429 => Err("Too many cape changes in a short time. Wait a minute, then try again.".into()),
        _ => Err(format!(
            "The Minecraft services couldn't change the cape (error {status}). Try again later."
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

    #[test]
    fn textures_are_fetched_over_https_only_from_mojang() {
        assert_eq!(
            https("http://textures.minecraft.net/texture/abc123").as_deref(),
            Some("https://textures.minecraft.net/texture/abc123")
        );
        for elsewhere in [
            "http://textures.minecraft.net.example.com/texture/abc",
            "http://example.com/texture/abc",
            "http://textures.minecraft.net/texture/../x",
            "http://textures.minecraft.net/texture/",
            "ftp://textures.minecraft.net/texture/abc",
        ] {
            assert_eq!(https(elsewhere), None, "{elsewhere}");
        }
    }

    #[test]
    fn a_body_that_is_not_a_profile_is_not_an_empty_one() {
        assert_eq!(Profile::of(&Value::Null), None);
        assert_eq!(Profile::of(&json!({})), None);
        assert_eq!(Profile::of(&json!({"capes": []})), None, "no id, no skins");
        let profile = Profile::of(&json!({"id": "x", "skins": []})).unwrap();
        assert_eq!(profile.skin, None);
        assert!(profile.capes.is_empty());
    }
}
