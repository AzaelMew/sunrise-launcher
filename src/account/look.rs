//! A Save on the Skins tab, as its worker does it: the skin (when it or its arms changed), then
//! the cape (when it changed), each reported on its own.
//!
//! - Nothing is written before the profile has been read with the token and found to be the
//!   account's own: a token for another account changes neither its skin nor its cape.
//! - Before a skin is replaced, the skin worn now (per that reading) is kept among the saved
//!   skins, with the arms it is worn with: the one actually replaced, not what an older copy of
//!   the profile said. When it can't be kept, the skin isn't replaced, so the failure shows
//!   before anything is lost.
//! - Changing only the arms keeps the picture the account wears when the Save starts. When that
//!   isn't the one the page showed (another launcher changed it meanwhile), nothing is sent, and
//!   the page is given the profile that shows the new one.
//! - A change the services refused is [`Change::Failed`] and stays to do. One they accepted is
//!   never sent again: [`Change::Done`] when the last profile of the Save shows it (for a skin,
//!   the picture served at its URL is the one sent, with its arms), else
//!   [`Change::Unconfirmed`].
//! - What was done is recorded in accounts.json before the Save reports, only the fields it
//!   changed: an account's Saves never overlap, so its records are written in order.
use super::capes::{self, Profile, Written};
use super::{skins, Endpoints};
use crate::skin_library::{self, Saved};
use std::path::PathBuf;

/// What a Save asks for.
#[derive(Clone, Debug)]
pub struct Request {
    /// The account it is for.
    pub uuid: String,
    /// The skin to wear, if it or its arms changed.
    pub skin: Option<SkinSent>,
    /// The cape to wear (by id) or none, if it changed.
    pub cape: Option<Option<String>>,
}

/// A skin to wear: its picture and its arms.
#[derive(Clone, Debug)]
pub struct SkinSent {
    pub picture: Picture,
    pub slim: bool,
}

/// The picture of a skin to wear.
#[derive(Clone, Debug)]
pub enum Picture {
    /// Another picture than the one worn; `saved` is the saved skin it is, if it is one.
    New { png: Vec<u8>, saved: Option<String> },
    /// The picture worn now, as the page knows it, with other arms.
    Worn(WornPicture),
}

/// The picture the page shows as worn: a skin on Mojang's texture server (its URL), or the
/// account's default skin (the picture the game has for it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WornPicture {
    Texture(String),
    Default(Vec<u8>),
}

/// One change of a Save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Accepted, and the last profile the Save read shows it.
    Done,
    /// Accepted, but no profile read after it shows it (why not): it isn't sent again, but nor
    /// is it what the account is known to wear.
    Unconfirmed(String),
    /// Refused or not sent (why).
    Failed(String),
}

impl Change {
    /// The services took it: it isn't sent again.
    pub fn accepted(&self) -> bool {
        !matches!(self, Change::Failed(_))
    }
}

/// The skin worn before a skin Save, kept among the saved skins.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kept {
    /// Nothing to keep: no skin was sent, the account wore a default skin, or the Save stopped
    /// before it got that far.
    Nothing,
    /// The skin at `url` is the saved skin `saved`.
    Kept { url: String, saved: Saved },
    /// It couldn't be kept (why), so the skin wasn't replaced.
    Failed(String),
}

/// What a Save did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub kept: Kept,
    /// The skin change; none when the skin wasn't changed.
    pub skin: Option<Change>,
    /// The cape change, the same way.
    pub cape: Option<Change>,
    /// The latest profile of the account the Save read or was sent: what it wears now, as far
    /// as is known. None when none could be had.
    pub profile: Option<Profile>,
    /// Why the arms of the saved skin worn couldn't be kept with it, if they couldn't.
    pub arms: Option<String>,
    /// Why accounts.json couldn't be told, if it couldn't.
    pub recorded: Option<String>,
}

/// What accounts.json is told a Save left the account wearing: each field none when the Save
/// didn't change it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Record {
    /// The skin's URL and model, or none for a default skin.
    pub skin: Option<Option<(String, String)>>,
    /// The cape's picture, or none for no cape.
    pub cape: Option<Option<String>>,
}

/// What a Save works with: the services, the account's token, its saved skins' folder, how a
/// picture is fetched from Mojang's texture server, and how accounts.json is told what changed.
pub struct Tools<'a> {
    pub ep: &'a Endpoints,
    pub token: Result<String, String>,
    pub library: Result<PathBuf, String>,
    pub fetch: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    pub record: &'a dyn Fn(&Record) -> Result<(), String>,
}

const OTHER_ACCOUNT: &str = "The Minecraft services answered with another account's profile.";
const CHANGED_ELSEWHERE: &str =
    "It was changed somewhere else since this page read it. The page now shows the skin you wear: check it, then Save again.";
const SKIN_NOT_SHOWN: &str =
    "The Minecraft services took the skin, but your profile doesn't show it yet.";
const SKIN_NOT_CHECKED: &str =
    "The Minecraft services took the skin, but its picture couldn't be fetched back to check it.";
const CAPE_NOT_SHOWN: &str =
    "The Minecraft services took the cape change, but your profile doesn't show it yet.";

/// Does what `request` asks.
pub fn save(request: &Request, tools: &Tools) -> Outcome {
    let mut out = Outcome {
        kept: Kept::Nothing,
        skin: None,
        cape: None,
        profile: None,
        arms: None,
        recorded: None,
    };
    // The token's profile, read before anything is written: whose it is, and what it wears.
    let token_and_profile = tools.token.clone().and_then(|token| {
        let before = capes::read(tools.ep, &token).and_then(|p| {
            p.is_for(&request.uuid)
                .then_some(p)
                .ok_or_else(|| OTHER_ACCOUNT.to_owned())
        })?;
        Ok((token, before))
    });
    let (token, before) = match token_and_profile {
        Ok(both) => both,
        Err(why) => {
            out.skin = request.skin.as_ref().map(|_| Change::Failed(why.clone()));
            out.cape = request.cape.as_ref().map(|_| Change::Failed(why));
            return out;
        }
    };
    out.profile = Some(before.clone());
    // The profile a write left, if it is this account's.
    let own = |written: Written| match written {
        Written::Profile(p) if p.is_for(&request.uuid) => Ok(p),
        Written::Profile(_) => Err(OTHER_ACCOUNT.to_owned()),
        Written::Unconfirmed(why) => Err(why),
    };
    // The picture uploaded and its arms, once the services took it: what a later profile is
    // checked against.
    let mut uploaded: Option<(Vec<u8>, bool)> = None;
    if let Some(sent) = &request.skin {
        let change = match upload_skin(sent, &before, tools, &mut out.kept) {
            Err(why) => Change::Failed(why),
            Ok(png) => {
                let written = skins::upload(tools.ep, &token, &png, sent.slim);
                match written {
                    Err(why) => Change::Failed(why),
                    Ok(written) => {
                        let change = match own(written) {
                            Ok(after) => {
                                let change = skin_shown(&after, &png, sent.slim, tools);
                                out.profile = Some(after);
                                change
                            }
                            Err(why) => Change::Unconfirmed(why),
                        };
                        uploaded = Some((png, sent.slim));
                        change
                    }
                }
            }
        };
        // The arms chosen for a saved skin are its own once the services took it with them.
        let saved = match (&sent.picture, &out.kept) {
            (Picture::New { saved, .. }, _) => saved.clone(),
            (Picture::Worn(_), Kept::Kept { saved, .. }) => Some(saved.id.clone()),
            (Picture::Worn(_), _) => None,
        };
        if let (true, Some(saved)) = (change.accepted(), saved) {
            out.arms = tools
                .library
                .clone()
                .and_then(|dir| skin_library::set_slim(&dir, &saved, sent.slim))
                .err();
        }
        out.skin = Some(change);
    }
    if let Some(cape) = &request.cape {
        out.cape = Some(match capes::wear(tools.ep, &token, cape.as_deref()) {
            Err(why) => Change::Failed(why),
            Ok(written) => match own(written) {
                Err(why) => Change::Unconfirmed(why),
                Ok(after) => {
                    // The latest profile says what the account wears: the skin taken before is
                    // checked against it again, so Done always means this profile shows it.
                    if let Some((png, slim)) = &uploaded {
                        out.skin = Some(skin_shown(&after, png, *slim, tools));
                    }
                    let change = if after.worn_cape().map(|c| &c.id) == cape.as_ref() {
                        Change::Done
                    } else {
                        Change::Unconfirmed(CAPE_NOT_SHOWN.into())
                    };
                    out.profile = Some(after);
                    change
                }
            },
        });
    }
    if let Some(profile) = &out.profile {
        let record = Record {
            skin: (out.skin == Some(Change::Done)).then(|| profile.skin.clone()),
            cape: (out.cape == Some(Change::Done))
                .then(|| profile.worn_cape().map(|c| c.url.clone())),
        };
        if record != Record::default() {
            out.recorded = (tools.record)(&record).err();
        }
    }
    out
}

/// The picture to upload for `sent`, once the skin `before` wears is kept (`kept` says how that
/// went); or why nothing is to be uploaded.
fn upload_skin(
    sent: &SkinSent,
    before: &Profile,
    tools: &Tools,
    kept: &mut Kept,
) -> Result<Vec<u8>, String> {
    // Only the arms: the account must still wear the picture the page showed.
    if let Picture::Worn(worn) = &sent.picture {
        let same = match (worn, &before.skin) {
            (WornPicture::Texture(shown), Some((url, _))) => shown == url,
            (WornPicture::Default(_), None) => true,
            _ => false,
        };
        if !same {
            return Err(CHANGED_ELSEWHERE.into());
        }
    }
    let current = match &before.skin {
        None => None,
        Some((url, model)) => {
            let Some(png) = (tools.fetch)(url) else {
                let why = "Its picture couldn't be downloaded from Mojang.".to_owned();
                *kept = Kept::Failed(why.clone());
                return Err(why);
            };
            match tools
                .library
                .clone()
                .and_then(|dir| skin_library::add(&dir, &png, "", Some(model == "slim")))
            {
                Ok(saved) => {
                    *kept = Kept::Kept {
                        url: url.clone(),
                        saved,
                    };
                }
                Err(why) => {
                    *kept = Kept::Failed(why.clone());
                    return Err(why);
                }
            }
            Some(png)
        }
    };
    Ok(match &sent.picture {
        Picture::New { png, .. } => png.clone(),
        Picture::Worn(WornPicture::Default(png)) => png.clone(),
        // Checked above: the account wears a skin from the texture server, just kept.
        Picture::Worn(WornPicture::Texture(_)) => current.ok_or(CHANGED_ELSEWHERE)?,
    })
}

/// Whether the profile `after` an upload shows the skin `png` with slim or classic arms: worn
/// with those arms, and the picture served at its URL is the same skin as the one sent (the
/// texture server sends another PNG of it, so the skins are compared, not the files).
fn skin_shown(after: &Profile, png: &[u8], slim: bool, tools: &Tools) -> Change {
    let Some((url, model)) = &after.skin else {
        return Change::Unconfirmed(SKIN_NOT_SHOWN.into());
    };
    if (model == "slim") != slim {
        return Change::Unconfirmed(SKIN_NOT_SHOWN.into());
    }
    let Some(served) = (tools.fetch)(url) else {
        return Change::Unconfirmed(SKIN_NOT_CHECKED.into());
    };
    match (
        skin_library::id_of_png(&served),
        skin_library::id_of_png(png),
    ) {
        (Some(served), Some(sent)) if served == sent => Change::Done,
        _ => Change::Unconfirmed(SKIN_NOT_SHOWN.into()),
    }
}
