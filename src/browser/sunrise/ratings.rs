//! Ratings of registry projects (the registry service's `/api/mods`, through the website).
//! Everyone reads the totals; a player rates, and reads back their own rating, with requests
//! signed by their Minecraft account's player certificate ([`crate::account::player_key`]),
//! so the registry never sees a token.
use crate::account::player_key::{self, PlayerKey};
use crate::i18n::tr;
use serde_json::{json, Value};
use std::collections::HashMap;

/// The registry API; development builds take `SUNRISE_MOD_API` instead, to see the ratings
/// against a local registry.
fn api() -> String {
    std::env::var("SUNRISE_MOD_API")
        .ok()
        .filter(|_| cfg!(debug_assertions))
        .unwrap_or_else(|| sunrise_launcher_addresses::MOD_API.to_owned())
}

/// A project's ratings: the average of its stars and how many players rated it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Totals {
    pub average: f64,
    pub count: u64,
}

fn totals_of(v: &Value) -> Totals {
    Totals {
        average: v["average"].as_f64().unwrap_or(0.0),
        count: v["count"].as_u64().unwrap_or(0),
    }
}

/// Every rated project's totals, by project ID.
pub fn totals() -> Result<HashMap<String, Totals>, String> {
    let (status, answer) = call("GET", &format!("{}/ratings", api()), None)?;
    if status != 200 {
        return Err(refusal(status));
    }
    Ok(answer["projects"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(id, v)| (id.clone(), totals_of(v)))
        .collect())
}

/// The player's own rating of `project`, if they rated it.
pub fn mine(project: &str) -> Result<Option<u8>, String> {
    let answer = signed("read", project, 0)?;
    Ok(answer["stars"]
        .as_u64()
        .and_then(|s| u8::try_from(s).ok())
        .filter(|s| (1..=5).contains(s)))
}

/// Rates `project` with `stars` (1 to 5), replacing the player's earlier rating; answers its new
/// totals.
pub fn rate(project: &str, stars: u8) -> Result<Totals, String> {
    Ok(totals_of(&signed("rate", project, stars)?))
}

/// The text a player signs for one request: the service's own first line, so the signature means
/// nothing anywhere else, then what it asks, for which project, and the registry's nonce.
fn signed_text(action: &str, project: &str, stars: u8, nonce: &str, timestamp: u64) -> String {
    format!("sunrise-rating-v1\n{action}\n{project}\n{stars}\n{nonce}\n{timestamp}")
}

/// A signed request's body: what was signed, the signature and the certificate's public part.
fn proof(
    key: &PlayerKey,
    action: &str,
    project: &str,
    stars: u8,
    nonce: &str,
    timestamp: u64,
) -> Result<Value, String> {
    let signature = key.sign(signed_text(action, project, stars, nonce, timestamp).as_bytes())?;
    Ok(json!({
        "action": action,
        "stars": stars,
        "nonce": nonce,
        "timestamp": timestamp,
        "signature": signature,
        "certificate": key.certificate(),
    }))
}

/// One signed request. A refusal of the proof is tried once more with a new nonce, and a new
/// certificate when the held one has expired (a nonce used or timed out, a certificate that ran
/// out on the way).
fn signed(action: &str, project: &str, stars: u8) -> Result<Value, String> {
    let api = api();
    let project_path = super::super::modrinth::encode(project);
    let url = match action {
        "read" => format!("{api}/{project_path}/rating/mine"),
        _ => format!("{api}/{project_path}/rating"),
    };
    let mut status = 0;
    for _ in 0..2 {
        let key = player_key::current()?;
        let (nonced, nonce) = call("POST", &format!("{api}/rating/nonce"), Some(json!({})))?;
        let Some(nonce) = nonce["nonce"].as_str().filter(|_| nonced == 200) else {
            // No nonce is a registry that can't take ratings now, whatever the project.
            return Err(refusal(503));
        };
        let timestamp = chrono::Utc::now().timestamp().max(0) as u64;
        let body = proof(&key, action, project, stars, nonce, timestamp)?;
        let (answered, answer) = call("POST", &url, Some(body))?;
        if answered == 200 {
            return Ok(answer);
        }
        status = answered;
        if answered != 403 {
            break;
        }
    }
    Err(refusal(status))
}

/// What the player reads for a refused request.
fn refusal(status: u16) -> String {
    match status {
        403 => tr("Couldn't confirm your Minecraft account.").into(),
        404 => tr("This mod isn't published in the registry.").into(),
        _ => tr("The registry can't take ratings right now. Try again later.").into(),
    }
}

fn call(method: &str, url: &str, body: Option<Value>) -> Result<(u16, Value), String> {
    let request = super::super::download::agent(url)?
        .request(method, url)
        .set("Accept", "application/json");
    let result = match body {
        Some(body) => request.send_json(body),
        None => request.call(),
    };
    let response = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(_)) => {
            return Err(tr("Couldn't reach the Sunrise registry.").into());
        }
    };
    let status = response.status();
    Ok((status, response.into_json().unwrap_or(Value::Null)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_request_carries_its_text_signed_by_the_certificate_key() {
        let key = player_key::tests::key();
        let body = proof(&key, "rate", "zoom", 4, "00ff", 1_795_448_000).unwrap();
        let text = "sunrise-rating-v1\nrate\nzoom\n4\n00ff\n1795448000";
        assert_eq!(signed_text("rate", "zoom", 4, "00ff", 1_795_448_000), text);
        assert!(key.verifies(text.as_bytes(), body["signature"].as_str().unwrap()));
        // Exactly the registry's fields: it refuses any other.
        let mut fields: Vec<_> = body.as_object().unwrap().keys().cloned().collect();
        fields.sort();
        assert_eq!(
            fields,
            [
                "action",
                "certificate",
                "nonce",
                "signature",
                "stars",
                "timestamp"
            ]
        );
        assert_eq!(body["certificate"], key.certificate());
        assert_eq!(
            (
                &body["action"],
                &body["stars"],
                &body["nonce"],
                &body["timestamp"]
            ),
            (
                &json!("rate"),
                &json!(4),
                &json!("00ff"),
                &json!(1_795_448_000u64)
            )
        );
    }
}
