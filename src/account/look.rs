//! A Save on the Skins tab, as its worker does it: the skin (when it or its arms changed), then
//! the cape (when it changed), each reported on its own.
//!
//! - Before a skin is replaced, the profile is read and the skin it wears now is kept among the
//!   saved skins: the one actually replaced, not what an older copy of the profile said. When it
//!   can't be kept (its picture can't be had, the saved skins can't be written), the skin isn't
//!   replaced, so the failure shows before anything is lost.
//! - A change the services refused is [`Change::Failed`] and stays to do. One they accepted is
//!   [`Change::Done`] when a profile after it shows it, and [`Change::Unconfirmed`] when none
//!   could be read: it isn't sent again, but nor is it what the account is known to wear.
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
    /// The skin to wear, if it changed.
    pub skin: Option<SkinSent>,
    /// The cape to wear (by id) or none, if it changed.
    pub cape: Option<Option<String>>,
}

/// A skin to wear: its picture, its arms, and the saved skin it is, if it is one.
#[derive(Clone, Debug)]
pub struct SkinSent {
    pub png: Vec<u8>,
    pub slim: bool,
    pub saved: Option<String>,
}

/// One change of a Save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    /// Accepted, and a profile read after it shows it.
    Done,
    /// Accepted, but no profile could be had after it (why not).
    Unconfirmed(String),
    /// Refused, not sent, or not shown by the profile after it (why).
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
    /// Nothing to keep: no skin was sent, or the account wore a default skin.
    Nothing,
    Kept(Saved),
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

/// What a Save works with: the services, the account's token, its saved skins' folder, how a
/// picture is fetched from Mojang's texture server, and how accounts.json is told what changed
/// (the skin and the cape; none leaves a field as it is).
pub struct Tools<'a> {
    pub ep: &'a Endpoints,
    pub token: Result<String, String>,
    pub library: Result<PathBuf, String>,
    pub fetch: &'a dyn Fn(&str) -> Option<Vec<u8>>,
    #[allow(clippy::type_complexity)]
    pub record:
        &'a dyn Fn(Option<Option<(String, String)>>, Option<Option<String>>) -> Result<(), String>,
}

const OTHER_ACCOUNT: &str = "The Minecraft services answered with another account's profile.";

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
    // The profile a write left, if it is this account's.
    let own = |written: Written| match written {
        Written::Profile(p) if p.is_for(&request.uuid) => Ok(p),
        Written::Profile(_) => Err(OTHER_ACCOUNT.to_owned()),
        Written::Unconfirmed(why) => Err(why),
    };
    // The skin sent and the profile before it, while the skin change awaits a profile after it.
    let mut skin_unconfirmed = None;
    if let Some(sent) = &request.skin {
        out.skin = Some(match &tools.token {
            Err(why) => Change::Failed(why.clone()),
            Ok(token) => {
                let before = capes::read(tools.ep, token).and_then(|p| {
                    p.is_for(&request.uuid)
                        .then_some(p)
                        .ok_or_else(|| OTHER_ACCOUNT.to_owned())
                });
                match before {
                    Err(why) => {
                        out.kept = Kept::Failed(why.clone());
                        Change::Failed(why)
                    }
                    Ok(before) => {
                        out.kept = keep(&before, tools);
                        out.profile = Some(before.clone());
                        match &out.kept {
                            Kept::Failed(why) => Change::Failed(why.clone()),
                            kept => match skins::upload(tools.ep, token, &sent.png, sent.slim) {
                                Err(why) => Change::Failed(why),
                                Ok(written) => match own(written) {
                                    Ok(after) => {
                                        let change = skin_shown(&before, kept, &after, sent);
                                        out.profile = Some(after);
                                        change
                                    }
                                    Err(why) => {
                                        skin_unconfirmed = Some((before, kept.clone()));
                                        Change::Unconfirmed(why)
                                    }
                                },
                            },
                        }
                    }
                }
            }
        });
        // The arms chosen for a saved skin are its own once the services took it with them.
        if let (Some(change), Some(saved)) = (&out.skin, &sent.saved) {
            if change.accepted() {
                out.arms = tools
                    .library
                    .clone()
                    .and_then(|dir| skin_library::set_slim(&dir, saved, sent.slim))
                    .err();
            }
        }
    }
    if let Some(cape) = &request.cape {
        out.cape = Some(match &tools.token {
            Err(why) => Change::Failed(why.clone()),
            Ok(token) => match capes::wear(tools.ep, token, cape.as_deref()) {
                Err(why) => Change::Failed(why),
                Ok(written) => match own(written) {
                    Ok(after) => {
                        // This profile comes after the skin change too, and says whether it took.
                        if let (Some((before, kept)), Some(sent)) =
                            (skin_unconfirmed.take(), &request.skin)
                        {
                            out.skin = Some(skin_shown(&before, &kept, &after, sent));
                        }
                        let change = if after.worn_cape().map(|c| &c.id) == cape.as_ref() {
                            Change::Done
                        } else {
                            Change::Failed(
                                "The Minecraft services took the cape change, but the profile still shows another cape.".into(),
                            )
                        };
                        out.profile = Some(after);
                        change
                    }
                    Err(why) => Change::Unconfirmed(why),
                },
            },
        });
    }
    if let Some(profile) = &out.profile {
        let skin = (out.skin == Some(Change::Done)).then(|| profile.skin.clone());
        let cape =
            (out.cape == Some(Change::Done)).then(|| profile.worn_cape().map(|c| c.url.clone()));
        if skin.is_some() || cape.is_some() {
            out.recorded = (tools.record)(skin, cape).err();
        }
    }
    out
}

/// Keeps the skin `before` wears among the saved skins, with the arms it wears it with.
fn keep(before: &Profile, tools: &Tools) -> Kept {
    let Some((url, model)) = &before.skin else {
        return Kept::Nothing;
    };
    let Some(png) = (tools.fetch)(url) else {
        return Kept::Failed("Its picture couldn't be downloaded from Mojang.".into());
    };
    match tools
        .library
        .clone()
        .and_then(|dir| skin_library::add(&dir, &png, "", Some(model == "slim")))
    {
        Ok(saved) => Kept::Kept(saved),
        Err(why) => Kept::Failed(why),
    }
}

/// Whether the profile `after` a skin upload shows the skin `sent`: worn, with its arms, and a
/// picture other than the one before unless the one before was the same skin (an arm change
/// keeps the URL).
fn skin_shown(before: &Profile, kept: &Kept, after: &Profile, sent: &SkinSent) -> Change {
    let shown = after.skin.as_ref().is_some_and(|(url, model)| {
        let same_picture = || match kept {
            Kept::Kept(saved) => crate::player_model::raw_skin(&sent.png)
                .is_some_and(|skin| skin_library::id_of(&skin) == saved.id),
            _ => false,
        };
        (model == "slim") == sent.slim
            && (before.skin.as_ref().map(|(u, _)| u) != Some(url) || same_picture())
    });
    if shown {
        Change::Done
    } else {
        Change::Failed(
            "The Minecraft services took the skin, but the profile still shows the one before."
                .into(),
        )
    }
}
