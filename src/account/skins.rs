//! The account's skin: a PNG and its arm model sent to the Minecraft services
//! (`POST /minecraft/profile/skins`, a multipart form with `variant` and `file`), which every
//! client and server then shows, as the official launcher does.
use super::capes::{self, Written};
use super::{bearer, request_to, Endpoints, Value};
use crate::i18n::{tr, trf};

/// The most a skin may weigh here: real ones are a few kilobytes.
pub const MAX_SKIN: usize = 256 * 1024;

/// The multipart body that sends `png` with slim or classic arms, between `boundary` lines.
pub(crate) fn form(boundary: &str, png: &[u8], slim: bool) -> Vec<u8> {
    let variant = if slim { "slim" } else { "classic" };
    let mut body = format!(
        "--{boundary}\r\nContent-Disposition: form-data; name=\"variant\"\r\n\r\n{variant}\r\n\
         --{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"skin.png\"\r\n\
         Content-Type: image/png\r\n\r\n"
    )
    .into_bytes();
    body.extend_from_slice(png);
    body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    body
}

/// Wears the skin `png` with slim or classic arms: what the change left, or why the services
/// refused it.
pub fn upload(ep: &Endpoints, token: &str, png: &[u8], slim: bool) -> Result<Written, String> {
    if png.len() > MAX_SKIN || crate::player_model::skin(png).is_none() {
        return Err(tr("This isn't a Minecraft skin: it must be a 64×64 or 64×32 PNG.").into());
    }
    let mut boundary = [0u8; 12];
    getrandom::fill(&mut boundary).map_err(|e| e.to_string())?;
    let boundary: String = boundary.iter().map(|b| format!("{b:02x}")).collect();
    let boundary = format!("sunrise-{boundary}");
    let request = bearer(
        request_to("POST", &format!("{}/minecraft/profile/skins", ep.minecraft))?,
        token,
    )
    .set(
        "Content-Type",
        &format!("multipart/form-data; boundary={boundary}"),
    );
    let response = match request.send_bytes(&form(&boundary, png, slim)) {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(_)) => {
            return Err(tr(
                "Could not reach the Minecraft services. Check your internet connection.",
            )
            .into())
        }
    };
    let status = response.status();
    let profile: Value =
        serde_json::from_str(&response.into_string().unwrap_or_default()).unwrap_or(Value::Null);
    match status {
        200 | 204 => Ok(capes::after_change(ep, token, &profile)),
        401 => Err(tr(super::SIGN_IN_AGAIN).into()),
        400 => Err(tr("Mojang refused this skin. It must be a 64×64 or 64×32 PNG.").into()),
        429 => {
            Err(tr("Too many skin changes in a short time. Wait a minute, then try again.").into())
        }
        _ => Err(trf(
            "The Minecraft services couldn't change the skin (error {status}). Try again later.",
            &[("status", &status.to_string())],
        )),
    }
}
