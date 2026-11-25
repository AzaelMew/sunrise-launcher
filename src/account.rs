//! Microsoft account sign-in (sunrise-org/sunrise's docs/microsoft-account.md): the OAuth device-code flow, then Xbox
//! Live, XSTS and the Minecraft services, the game ownership check and the profile. Secrets (the
//! Microsoft refresh token, the Minecraft access token, the last ownership answer) live in the OS
//! keychain, or in a file only the player can read where there is none; accounts.json keeps only
//! names and UUIDs. The game gets the session in a file it
//! reads once (`--session-file`), never on its command line. Tokens never reach logs, errors or
//! output.
//!
//! The owner gate ([`launch`]): the launcher starts the game only while an account in it owns
//! Minecraft: Java Edition, as other launchers do, and the game plays as the selected account.
//! The game binary itself, its harnesses and the servers don't go through the launcher's gate.
//!
//! The Azure app (client ID) is configuration read at build time: `SUNRISE_MSA_CLIENT_ID`
//! (release builds take it from the repository variable), which the same variable at run time
//! overrides. Without one, sign-in says it is not configured.
pub mod capes;
pub mod look;
pub mod player_key;
mod secret_store;
mod secrets_file;
pub mod session;
pub mod skins;
mod system_keychain;
pub use secret_store::*;
pub use secrets_file::*;
pub use system_keychain::*;

use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{SocketAddr, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The keychain service name every secret is stored under.
pub const SERVICE: &str = crate::identity::KEYCHAIN_SERVICE;
const SCOPE: &str = "XboxLive.signin offline_access";
pub const NOT_CONFIGURED: &str = "This build of the launcher has no Microsoft sign-in (SUNRISE_MSA_CLIENT_ID), so it can't start the game.";
/// What every Play says while no account in the launcher owns the game.
pub const NEEDS_OWNER: &str =
    "Sign in with a Microsoft account that owns Minecraft: Java Edition to play";
const DOES_NOT_OWN: &str = "This account doesn't own Minecraft: Java Edition.";
const NO_PROFILE: &str =
    "This account has no Minecraft profile yet. Create one at minecraft.net, then sign in again.";
const OTHER_PROFILE: &str =
    "The Microsoft account now has another Minecraft profile. Sign in again.";
const SIGN_IN_AGAIN: &str = "Your Microsoft sign-in expired. Sign in again.";
/// The Minecraft token is renewed when it has less than this left.
const RENEW_BEFORE: u64 = 3600;

/// The configured Azure app, if any: the run-time variable wins over the build-time one, and an
/// empty value means none.
pub fn client_id() -> Option<String> {
    std::env::var("SUNRISE_MSA_CLIENT_ID")
        .ok()
        .or_else(|| option_env!("SUNRISE_MSA_CLIENT_ID").map(str::to_owned))
        .map(|id| id.trim().to_owned())
        .filter(|id| !id.is_empty())
}

/// Where the requests go: the real services, or a mock in tests.
#[derive(Clone, Debug)]
pub struct Endpoints {
    /// The OAuth 2.0 v2 endpoints for personal accounts (`…/devicecode`, `…/token`).
    pub microsoft: String,
    pub xbox_user: String,
    pub xsts: String,
    /// api.minecraftservices.com.
    pub minecraft: String,
}

impl Endpoints {
    pub fn live() -> Self {
        // Development builds only: `SUNRISE_MSA_MICROSOFT_ENDPOINT` points the device-code and
        // token requests at a local mock, to see and screenshot the sign-in panel.
        let microsoft = std::env::var("SUNRISE_MSA_MICROSOFT_ENDPOINT")
            .ok()
            .filter(|_| cfg!(debug_assertions));
        // The same for the Minecraft services (the profile, its skin and capes), to see and
        // screenshot the Skins tab against a local mock.
        let minecraft = std::env::var("SUNRISE_MINECRAFT_SERVICES_ENDPOINT")
            .ok()
            .filter(|_| cfg!(debug_assertions));
        Self::at(microsoft.as_deref(), minecraft.as_deref())
    }

    /// The live endpoints, taking the development mocks as parameters so the decision does not
    /// depend on the process environment tests share.
    fn at(microsoft: Option<&str>, minecraft: Option<&str>) -> Self {
        Self {
            microsoft: microsoft
                .unwrap_or(sunrise_launcher_addresses::MICROSOFT_LOGIN)
                .into(),
            xbox_user: sunrise_launcher_addresses::XBOX_USER_AUTH.into(),
            xsts: sunrise_launcher_addresses::XSTS_AUTH.into(),
            minecraft: minecraft
                .unwrap_or(sunrise_launcher_addresses::MINECRAFT_SERVICES)
                .into(),
        }
    }
}

/// Orders resolved addresses so the two families alternate, the first one's address first (the
/// order RFC 8305 asks a client to try them in). A host whose IPv6 route is dead then costs one
/// short attempt before its IPv4 address is tried, instead of every AAAA record in turn.
fn interleave(addrs: Vec<SocketAddr>) -> Vec<SocketAddr> {
    let Some(first) = addrs.first().map(SocketAddr::is_ipv6) else {
        return addrs;
    };
    let (mut same, mut other): (Vec<_>, Vec<_>) =
        addrs.into_iter().partition(|a| a.is_ipv6() == first);
    same.reverse();
    other.reverse();
    let mut out = Vec::with_capacity(same.len() + other.len());
    while !same.is_empty() || !other.is_empty() {
        out.extend(same.pop());
        out.extend(other.pop());
    }
    out
}

/// The system's resolver with its answer interleaved (`interleave`).
struct Interleaved<R>(R);

impl<R: ureq::Resolver> ureq::Resolver for Interleaved<R> {
    fn resolve(&self, netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
        self.0.resolve(netloc).map(interleave)
    }
}

struct System;

impl ureq::Resolver for System {
    fn resolve(&self, netloc: &str) -> std::io::Result<Vec<SocketAddr>> {
        netloc.to_socket_addrs().map(Iterator::collect)
    }
}

/// Keep the short connect timeout and interleaved addresses so a dead IPv6 route
/// falls back to IPv4 before the whole sign-in request times out.
fn sign_in_agent(builder: ureq::AgentBuilder) -> ureq::AgentBuilder {
    builder
        .timeout(Duration::from_secs(20))
        .timeout_connect(Duration::from_secs(10))
        .resolver(Interleaved(System))
        .user_agent(crate::proxy::USER_AGENT)
}

fn request_to(method: &str, url: &str) -> Result<ureq::Request, String> {
    Ok(crate::proxy::agent("account", url, sign_in_agent)?.request(method, url))
}

/// One request's result: the status and the JSON body (empty when there is none). Transport
/// errors name the service, never the URL's query or the request.
fn send(request: ureq::Request, body: Option<Body>, service: &str) -> Result<(u16, Value), String> {
    let result = match body {
        None => request.call(),
        Some(Body::Form(fields)) => request.send_form(&fields),
        Some(Body::Json(value)) => request.set("Accept", "application/json").send_json(value),
    };
    let response = match result {
        Ok(r) => r,
        Err(ureq::Error::Status(_, r)) => r,
        Err(ureq::Error::Transport(_)) => {
            return Err(format!(
                "Could not reach {service}. Check your internet connection."
            ));
        }
    };
    let status = response.status();
    let text = response.into_string().unwrap_or_default();
    Ok((status, serde_json::from_str(&text).unwrap_or(Value::Null)))
}

enum Body<'a> {
    Form(Vec<(&'a str, &'a str)>),
    Json(Value),
}

fn str_field(v: &Value, key: &str) -> Option<String> {
    v.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

// ----- The device-code flow ----------------------------------------------------------------------

/// What the player types at the verification page.
#[derive(Clone, PartialEq)]
pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_at: u64,
    device_code: String,
}

impl std::fmt::Debug for DeviceCode {
    // The device code can be traded for tokens once the player approves: never shown.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceCode")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .finish_non_exhaustive()
    }
}

/// The Microsoft tokens; the access token only lasts for the Xbox step.
pub struct MsTokens {
    access: String,
    refresh: String,
}

impl std::fmt::Debug for MsTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MsTokens(<hidden>)")
    }
}

pub enum Poll {
    Pending,
    /// The server asked to poll less often: wait 5 s more.
    SlowDown,
    Done(MsTokens),
}

pub fn device_code(ep: &Endpoints, client_id: &str) -> Result<DeviceCode, String> {
    let (status, v) = send(
        request_to("POST", &format!("{}/devicecode", ep.microsoft))?,
        Some(Body::Form(vec![("client_id", client_id), ("scope", SCOPE)])),
        "Microsoft",
    )?;
    match (
        status,
        str_field(&v, "user_code"),
        str_field(&v, "device_code"),
    ) {
        (200, Some(user_code), Some(device_code)) => Ok(DeviceCode {
            user_code,
            verification_uri: str_field(&v, "verification_uri")
                .unwrap_or_else(|| sunrise_launcher_addresses::MICROSOFT_LINK.into()),
            interval: v
                .get("interval")
                .and_then(Value::as_u64)
                .unwrap_or(5)
                .clamp(1, 60),
            // The service's numbers, bounded: Instant + a huge interval panics the sign-in thread.
            expires_at: now().saturating_add(
                v.get("expires_in")
                    .and_then(Value::as_u64)
                    .unwrap_or(900)
                    .min(3600),
            ),
            device_code,
        }),
        _ => Err(oauth_error(&v)),
    }
}

/// The OAuth error codes as messages for the player.
fn oauth_error(v: &Value) -> String {
    match v.get("error").and_then(Value::as_str) {
        Some("authorization_declined") => "Sign-in was declined in the browser.".into(),
        Some("expired_token") => "The sign-in code expired. Start again.".into(),
        Some("bad_verification_code") => "The sign-in code was not recognised. Start again.".into(),
        Some("invalid_grant") => SIGN_IN_AGAIN.into(),
        Some("invalid_client" | "unauthorized_client") => {
            "Microsoft refused this launcher's app registration (client ID).".into()
        }
        Some(other) => format!("Microsoft sign-in failed ({other})."),
        None => "Microsoft sign-in failed.".into(),
    }
}

/// One poll of the token endpoint while the player signs in.
pub fn poll(ep: &Endpoints, client_id: &str, code: &DeviceCode) -> Result<Poll, String> {
    let (status, v) = send(
        request_to("POST", &format!("{}/token", ep.microsoft))?,
        Some(Body::Form(vec![
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ("client_id", client_id),
            ("device_code", &code.device_code),
        ])),
        "Microsoft",
    )?;
    match v.get("error").and_then(Value::as_str) {
        Some("authorization_pending") => return Ok(Poll::Pending),
        Some("slow_down") => return Ok(Poll::SlowDown),
        _ => {}
    }
    ms_tokens(status, &v).map(Poll::Done)
}

fn ms_tokens(status: u16, v: &Value) -> Result<MsTokens, String> {
    match (
        status,
        str_field(v, "access_token"),
        str_field(v, "refresh_token"),
    ) {
        (200, Some(access), Some(refresh)) => Ok(MsTokens { access, refresh }),
        _ => Err(oauth_error(v)),
    }
}

/// Wait for the player to finish in the browser, polling as asked, until the code expires or
/// `cancelled` says stop.
pub fn wait_for_sign_in(
    ep: &Endpoints,
    client_id: &str,
    code: &DeviceCode,
    cancelled: &dyn Fn() -> bool,
) -> Result<MsTokens, String> {
    let mut interval = code.interval;
    loop {
        let until = std::time::Instant::now() + Duration::from_secs(interval);
        while std::time::Instant::now() < until {
            if cancelled() {
                return Err("Sign-in cancelled.".into());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if now() > code.expires_at {
            return Err("The sign-in code expired. Start again.".into());
        }
        match poll(ep, client_id, code)? {
            Poll::Pending => {}
            Poll::SlowDown => interval = (interval + 5).min(60),
            Poll::Done(tokens) => return Ok(tokens),
        }
    }
}

fn refresh_ms(ep: &Endpoints, client_id: &str, refresh: &str) -> Result<MsTokens, String> {
    let (status, v) = send(
        request_to("POST", &format!("{}/token", ep.microsoft))?,
        Some(Body::Form(vec![
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", refresh),
            ("scope", SCOPE),
        ])),
        "Microsoft",
    )?;
    // A refresh token issued to another app registration (the launcher's client ID changed
    // since the player signed in) is refused as the client's; only a new sign-in mends it.
    match v.get("error").and_then(Value::as_str) {
        Some("invalid_client" | "unauthorized_client") => Err(SIGN_IN_AGAIN.into()),
        _ => ms_tokens(status, &v),
    }
}

// ----- Xbox Live, XSTS, Minecraft ----------------------------------------------------------------

const FAMILY_URL: &str = sunrise_launcher_addresses::MICROSOFT_FAMILY;
const CHILD_ACCOUNT: &str = "Microsoft says this is a child account (error {code}). An adult has to add it to a Microsoft family at {url}. If it is an adult account, check its family settings there.";

/// The XSTS `XErr` codes, as the official launcher explains them.
fn xsts_error(code: u64) -> String {
    match code {
        2148916233 => "This Microsoft account has no Xbox profile yet. Sign in once at xbox.com to create one, then try again.".into(),
        2148916235 => "Xbox Live is not available in your country or region.".into(),
        2148916236 | 2148916237 => "This account needs adult verification on xbox.com before it can play.".into(),
        2148916238 => crate::i18n::trf(
            CHILD_ACCOUNT,
            &[("code", &code.to_string()), ("url", FAMILY_URL)],
        ),
        other => format!("Xbox Live refused the sign-in (error {other})."),
    }
}

/// The Minecraft access token from the Microsoft one, through Xbox Live and XSTS.
struct Minecraft {
    token: String,
    expires_at: u64,
    xuid: Option<String>,
}

fn minecraft_login(
    ep: &Endpoints,
    ms_access: &str,
    progress: &dyn Fn(session::Phase),
) -> Result<Minecraft, String> {
    progress(session::Phase::Xbox);
    let (status, xbl) = send(
        request_to("POST", &ep.xbox_user)?,
        Some(Body::Json(json!({
            "Properties": {
                "AuthMethod": "RPS",
                "SiteName": "user.auth.xboxlive.com",
                "RpsTicket": format!("d={ms_access}"),
            },
            "RelyingParty": "http://auth.xboxlive.com",
            "TokenType": "JWT",
        }))),
        "Xbox Live",
    )?;
    let xbl_token = str_field(&xbl, "Token")
        .filter(|_| status == 200)
        .ok_or("Xbox Live refused the sign-in.")?;
    let (status, xsts) = send(
        request_to("POST", &ep.xsts)?,
        Some(Body::Json(json!({
            "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] },
            "RelyingParty": "rp://api.minecraftservices.com/",
            "TokenType": "JWT",
        }))),
        "Xbox Live",
    )?;
    if let Some(code) = xsts.get("XErr").and_then(Value::as_u64) {
        return Err(xsts_error(code));
    }
    let claims = xsts.pointer("/DisplayClaims/xui/0");
    let (Some(xsts_token), Some(uhs)) = (
        str_field(&xsts, "Token").filter(|_| status == 200),
        claims.and_then(|c| str_field(c, "uhs")),
    ) else {
        return Err("Xbox Live refused the sign-in.".into());
    };
    let xuid = claims.and_then(|c| str_field(c, "xid"));
    progress(session::Phase::Minecraft);
    let (token, expires_in) = minecraft_token(ep, &format!("XBL3.0 x={uhs};{xsts_token}"))?;
    Ok(Minecraft {
        token,
        expires_at: now().saturating_add(expires_in.min(7 * 86_400)),
        xuid,
    })
}

/// The Minecraft access token and its lifetime from the XSTS one (`XBL3.0 x=<uhs>;<token>`).
/// `launcher/login` first: a new account's token from `login_with_xbox` names no profile, so
/// joining an online-mode server answers "Profile not found" (found and diagnosed by
/// martipops). `login_with_xbox` stays the fallback, so a sign-in that worked before still does
/// when the other refuses.
fn minecraft_token(ep: &Endpoints, xtoken: &str) -> Result<(String, u64), String> {
    let lifetime = |v: &Value| v.get("expires_in").and_then(Value::as_u64).unwrap_or(86400);
    let launcher = send(
        request_to("POST", &format!("{}/launcher/login", ep.minecraft))?,
        Some(Body::Json(
            json!({ "xtoken": xtoken, "platform": "PC_LAUNCHER" }),
        )),
        "the Minecraft services",
    );
    if let Ok((200, v)) = &launcher {
        if let Some(token) = str_field(v, "access_token") {
            return Ok((token, lifetime(v)));
        }
    }
    let (status, mc) = send(
        request_to(
            "POST",
            &format!("{}/authentication/login_with_xbox", ep.minecraft),
        )?,
        Some(Body::Json(json!({ "identityToken": xtoken }))),
        "the Minecraft services",
    )?;
    match (status, str_field(&mc, "access_token")) {
        (200, Some(token)) => Ok((token, lifetime(&mc))),
        // New Azure apps are refused until Mojang approves them (Minecraft API access).
        (403, _) => Err(
            "The Minecraft services refused this launcher: its app is not approved by Mojang yet."
                .into(),
        ),
        (429, _) => Err("Too many sign-ins in a short time. Wait a minute and try again.".into()),
        _ => Err("The Minecraft services refused the sign-in.".into()),
    }
}

fn bearer(request: ureq::Request, token: &str) -> ureq::Request {
    request.set("Authorization", &format!("Bearer {token}"))
}

/// Whether the account owns Minecraft: Java Edition (Game Pass included).
fn owns_game(ep: &Endpoints, token: &str) -> Result<bool, String> {
    let (status, v) = send(
        bearer(
            request_to("GET", &format!("{}/entitlements/mcstore", ep.minecraft))?,
            token,
        ),
        None,
        "the Minecraft services",
    )?;
    if status != 200 {
        return Err("Could not check which games this account owns.".into());
    }
    Ok(v.get("items")
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items.iter().any(|i| {
                matches!(
                    i.get("name").and_then(Value::as_str),
                    Some("game_minecraft" | "product_minecraft")
                )
            })
        }))
}

/// A Minecraft profile, as the launcher shows and remembers it (no secrets).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Account {
    /// The profile UUID without dashes.
    pub uuid: String,
    pub name: String,
    /// The active skin's URL (textures.minecraft.net) and model ("classic" or "slim").
    #[serde(default)]
    pub skin: Option<(String, String)>,
    #[serde(default)]
    pub cape: Option<String>,
}

/// The profile; none when the account has none (it doesn't own the game, or never made one).
fn profile(ep: &Endpoints, token: &str) -> Result<Option<Account>, String> {
    let (status, v) = send(
        bearer(
            request_to("GET", &format!("{}/minecraft/profile", ep.minecraft))?,
            token,
        ),
        None,
        "the Minecraft services",
    )?;
    if status == 404 {
        return Ok(None);
    }
    let (Some(uuid), Some(name)) = (str_field(&v, "id"), str_field(&v, "name")) else {
        return Err("Could not read the Minecraft profile.".into());
    };
    let active = |key: &str| {
        v.get(key)
            .and_then(Value::as_array)
            .and_then(|list| {
                list.iter()
                    .find(|s| s.get("state").and_then(Value::as_str) == Some("ACTIVE"))
            })
            .cloned()
    };
    Ok(Some(Account {
        uuid: uuid.replace('-', "").to_lowercase(),
        name,
        skin: active("skins").and_then(|s| {
            Some((
                str_field(&s, "url")?,
                str_field(&s, "variant")
                    .unwrap_or_else(|| "CLASSIC".into())
                    .to_lowercase(),
            ))
        }),
        cape: active("capes").and_then(|c| str_field(&c, "url")),
    }))
}

/// What the keychain keeps for one account.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Secrets {
    refresh_token: String,
    #[serde(default)]
    sign_in_required: bool,
    minecraft_token: String,
    minecraft_expires_at: u64,
    #[serde(default)]
    xuid: Option<String>,
    /// The last answer the Minecraft services gave: whether the account owns the game. It is
    /// kept here, beside the tokens, so that no plain file can say yes. Accounts saved before it
    /// existed passed the same check at sign-in, which refuses everyone else.
    #[serde(default = "owned_at_sign_in")]
    owns: bool,
}
fn owned_at_sign_in() -> bool {
    true
}
impl std::fmt::Debug for Secrets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secrets(<hidden>)")
    }
}

/// From Microsoft tokens to a ready account: Xbox, XSTS, Minecraft, ownership, profile.
pub fn finish_sign_in(ep: &Endpoints, ms: MsTokens) -> Result<(Account, Secrets), String> {
    finish_sign_in_with_progress(ep, ms, &|_| {})
}

pub fn finish_sign_in_with_progress(
    ep: &Endpoints,
    ms: MsTokens,
    progress: &dyn Fn(session::Phase),
) -> Result<(Account, Secrets), String> {
    let mc = minecraft_login(ep, &ms.access, progress)?;
    progress(session::Phase::Ownership);
    if !owns_game(ep, &mc.token)? {
        return Err(DOES_NOT_OWN.into());
    }
    progress(session::Phase::Profile);
    let account = profile(ep, &mc.token)?.ok_or(NO_PROFILE)?;
    Ok((account, Secrets::new(ms.refresh, mc)))
}

impl Secrets {
    fn new(refresh_token: String, mc: Minecraft) -> Self {
        Self {
            refresh_token,
            sign_in_required: false,
            minecraft_token: mc.token,
            minecraft_expires_at: mc.expires_at,
            xuid: mc.xuid,
            owns: true,
        }
    }
}

/// What asking the Minecraft services again found for one account.
enum Check {
    /// It owns the game: its profile as it is now (the name may have changed) and its tokens.
    Owns(Account, Secrets),
    /// The services said it doesn't own the game (or has no profile any more).
    DoesNot(Secrets),
    /// No answer (no network, a sign-in that can't be renewed): the last answer stands.
    NoAnswer(String, Secrets),
}

/// Asks whether `account` owns the game, with a Minecraft token renewed through the refresh token
/// first when it has less than an hour left. The secrets come back renewed whatever the answer.
fn check(ep: &Endpoints, client_id: Option<&str>, account: &Account, secrets: Secrets) -> Check {
    check_with(ep, client_id, account, secrets, false, &|_| {})
}

fn check_with(
    ep: &Endpoints,
    client_id: Option<&str>,
    account: &Account,
    mut secrets: Secrets,
    force: bool,
    progress: &dyn Fn(session::Phase),
) -> Check {
    if secrets.sign_in_required {
        return Check::NoAnswer(SIGN_IN_AGAIN.into(), secrets);
    }
    if force || secrets.minecraft_expires_at <= now() + RENEW_BEFORE {
        progress(session::Phase::Microsoft);
        let renewed = client_id
            .ok_or_else(|| NOT_CONFIGURED.to_string())
            .and_then(|id| refresh_ms(ep, id, &secrets.refresh_token));
        let ms = match renewed {
            Ok(ms) => ms,
            Err(e) => {
                secrets.sign_in_required = e == SIGN_IN_AGAIN;
                return Check::NoAnswer(e, secrets);
            }
        };
        // Microsoft can rotate its refresh credential even if a later service fails.
        secrets.refresh_token = ms.refresh;
        match minecraft_login(ep, &ms.access, progress) {
            Ok(mc) => {
                secrets = Secrets {
                    owns: secrets.owns,
                    ..Secrets::new(secrets.refresh_token, mc)
                }
            }
            Err(e) => return Check::NoAnswer(e, secrets),
        }
    }
    let token = &secrets.minecraft_token;
    progress(session::Phase::Ownership);
    match owns_game(ep, token).and_then(|owns| {
        if !owns {
            return Ok(None);
        }
        progress(session::Phase::Profile);
        profile(ep, token)
    }) {
        Err(e) => Check::NoAnswer(e, secrets),
        Ok(None) => Check::DoesNot(Secrets {
            owns: false,
            ..secrets
        }),
        Ok(Some(p)) if p.uuid != account.uuid => Check::NoAnswer(
            OTHER_PROFILE.into(),
            Secrets {
                sign_in_required: true,
                ..secrets
            },
        ),
        Ok(Some(p)) => Check::Owns(
            p,
            Secrets {
                owns: true,
                ..secrets
            },
        ),
    }
}

// ----- Accounts ------------------------------------------------------------------------------------

/// accounts.json: the signed-in accounts and the one that plays. Never a token.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Accounts {
    pub accounts: Vec<Account>,
    /// The UUID of the account the game starts with; none: the first one.
    pub selected: Option<String>,
}

/// The secrets an account keeps in `store`, if it still has them there.
fn secrets_in(store: &dyn SecretStore, uuid: &str) -> Option<Secrets> {
    store
        .get(uuid)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
}

impl Accounts {
    pub(crate) fn path() -> PathBuf {
        crate::backend::data_dir().join("accounts.json")
    }
    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }
    pub fn save(&self) -> Result<(), String> {
        self.save_to(&Self::path())
    }
    pub(crate) fn load_from(path: &Path) -> Self {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }
    pub(crate) fn save_to(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        // A new file renamed over the old one: a crash or a full disk mid-write leaves the old list,
        // not an empty file that reads as no accounts at all.
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        if let Err(e) = std::fs::write(&tmp, text + "\n").and_then(|()| std::fs::rename(&tmp, path))
        {
            let _ = std::fs::remove_file(&tmp);
            return Err(e.to_string());
        }
        Ok(())
    }
    /// Saves what a launch renewed (names, skins) into `accounts.json` as it is now, under the
    /// accounts' lock: an account signed in or out while the services were asked stays so, where
    /// saving this copy, loaded before, would undo it.
    pub fn save_renewed(&self) -> Result<(), String> {
        self.save_renewed_to(store(), &Self::path())
    }
    pub(crate) fn save_renewed_to(
        &self,
        store: &dyn SecretStore,
        path: &Path,
    ) -> Result<(), String> {
        let _operation = session::lock(store)?;
        let mut saved = Self::load_from(path);
        for account in &self.accounts {
            saved.update(account.clone());
        }
        if saved.selected().is_none() {
            saved.selected = self
                .selected
                .clone()
                .filter(|u| saved.accounts.iter().any(|a| &a.uuid == u));
        }
        saved.save_to(path)
    }
    /// Records what a Save on the Skins tab left account `uuid` wearing, in `accounts.json` as it
    /// is now and under the accounts' lock: only the fields that Save changed (`skin`, `cape`;
    /// none leaves a field as it is), so nothing else about that account or any other is
    /// written back from an older copy.
    pub fn save_look(
        uuid: &str,
        skin: Option<Option<(String, String)>>,
        cape: Option<Option<String>>,
    ) -> Result<(), String> {
        Self::save_look_to(store(), &Self::path(), uuid, skin, cape)
    }
    pub(crate) fn save_look_to(
        store: &dyn SecretStore,
        path: &Path,
        uuid: &str,
        skin: Option<Option<(String, String)>>,
        cape: Option<Option<String>>,
    ) -> Result<(), String> {
        let _operation = session::lock(store)?;
        let mut saved = Self::load_from(path);
        // Signed out meanwhile: there is nothing left to record it on.
        let Some(account) = saved.accounts.iter_mut().find(|a| a.uuid == uuid) else {
            return Ok(());
        };
        if let Some(skin) = skin {
            account.skin = skin;
        }
        if let Some(cape) = cape {
            account.cape = cape;
        }
        saved.save_to(path)
    }
    pub fn selected(&self) -> Option<&Account> {
        let uuid = self.selected.as_ref()?;
        self.accounts.iter().find(|a| &a.uuid == uuid)
    }
    /// Find an account by name (any case) or UUID (with or without dashes).
    pub fn find(&self, who: &str) -> Option<&Account> {
        let undashed = who.replace('-', "").to_lowercase();
        self.accounts
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(who) || a.uuid == undashed)
    }
    /// Keep a new or renewed account, its secrets in the store, and select it.
    pub fn add(
        &mut self,
        store: &dyn SecretStore,
        account: Account,
        secrets: &Secrets,
    ) -> Result<(), String> {
        let _operation = session::lock(store)?;
        self.keep_sign_in(store, account, secrets)
    }

    fn keep_sign_in(
        &mut self,
        store: &dyn SecretStore,
        account: Account,
        secrets: &Secrets,
    ) -> Result<(), String> {
        store.set(
            &account.uuid,
            &serde_json::to_string(secrets).map_err(|e| e.to_string())?,
        )?;
        self.selected = Some(account.uuid.clone());
        match self.accounts.iter_mut().find(|a| a.uuid == account.uuid) {
            Some(a) => *a = account,
            None => self.accounts.push(account),
        }
        Ok(())
    }
    /// Whether an account here owned the game at its last check (the keychain's answer, no
    /// request): what Play needs.
    pub fn has_owner(&self, store: &dyn SecretStore) -> bool {
        // The selected account first: it owning the game ends the search after one account's
        // item — every account's item read can ask for the keychain (macOS, #4912).
        self.selected()
            .into_iter()
            .chain(self.accounts.iter().filter(|a| Some(*a) != self.selected()))
            .any(|a| secrets_in(store, &a.uuid).is_some_and(|s| s.owns))
    }

    /// Plays as the signed-in account `uuid`.
    pub fn play_as(&mut self, uuid: &str) -> Result<(), String> {
        if !self.accounts.iter().any(|a| a.uuid == uuid) {
            return Err("That account isn't signed in.".into());
        }
        self.selected = Some(uuid.to_string());
        Ok(())
    }

    /// Switches who plays to `uuid`, on disk: the list as it is saved now with that account
    /// selected, saved again. The window keeps what this returns.
    pub fn switch(uuid: &str) -> Result<Self, String> {
        Self::switch_in(&Self::path(), uuid)
    }
    pub(crate) fn switch_in(path: &Path, uuid: &str) -> Result<Self, String> {
        let mut accounts = Self::load_from(path);
        accounts.play_as(uuid)?;
        accounts.save_to(path)?;
        Ok(accounts)
    }

    /// Moves `uuid` by `delta` (`-1` up, `+1` down) in the account list. Returns whether the order
    /// changed.
    pub fn move_account(&mut self, uuid: &str, delta: isize) -> bool {
        let Some(from) = self.accounts.iter().position(|a| a.uuid == uuid) else {
            return false;
        };
        let Some(to) = from
            .checked_add_signed(delta)
            .filter(|&i| i < self.accounts.len())
        else {
            return false;
        };
        if from == to {
            return false;
        }
        self.accounts.swap(from, to);
        true
    }

    /// Reorders `uuid` by `delta` on disk and returns the saved list.
    pub fn reorder(uuid: &str, delta: isize) -> Result<Self, String> {
        Self::reorder_in(&Self::path(), uuid, delta)
    }
    pub(crate) fn reorder_in(path: &Path, uuid: &str, delta: isize) -> Result<Self, String> {
        let mut accounts = Self::load_from(path);
        if accounts.move_account(uuid, delta) {
            accounts.save_to(path)?;
        }
        Ok(accounts)
    }

    /// A renewed profile (its name or skin changed) in place of the old one; who plays stays.
    fn update(&mut self, account: Account) {
        if let Some(a) = self.accounts.iter_mut().find(|a| a.uuid == account.uuid) {
            *a = account;
        }
    }

    /// Sign out: the secrets leave the keychain, the account the list.
    pub fn remove(&mut self, store: &dyn SecretStore, uuid: &str) -> Result<(), String> {
        let _operation = session::lock(store)?;
        store.delete(uuid)?;
        self.accounts.retain(|a| a.uuid != uuid);
        if self.selected.as_deref() == Some(uuid) {
            self.selected = None;
        }
        Ok(())
    }
}

/// No account in the launcher owns Minecraft: Java Edition: the game must not start.
#[derive(Debug, Default, PartialEq)]
pub struct NoOwner {
    /// Why a sign-in couldn't be read, when the keyring didn't give it (#12179), or why the
    /// account that plays can't: what is said instead of the refusal, which would tell a
    /// signed-in player to sign in.
    pub keyring: Option<String>,
}
impl std::fmt::Display for NoOwner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.keyring {
            Some(why) => write!(f, "{why}"),
            None => write!(f, "{NEEDS_OWNER}."),
        }
    }
}

impl NoOwner {
    /// The same refusal for the window, which has Settings rather than a command to name.
    pub fn in_window(&self) -> String {
        use crate::i18n::tr;
        if let Some(why) = &self.keyring {
            return why.clone();
        }
        format!(
            "{}. {}",
            tr(NEEDS_OWNER),
            tr("Open Settings and sign in under Account.")
        )
    }
}

/// The owner gate's pass for one launch.
#[derive(Debug, Default)]
pub struct Launch {
    /// The selected account and its renewed session; none when its sign-in couldn't be renewed
    /// now (`offline` says why).
    pub session: Option<(Account, Secrets)>,
    /// Why the selected account plays without its session this time.
    pub offline: Option<String>,
    /// An account the services said owns the game just now, the selected one first: who the
    /// launcher may download the game for.
    pub owner: Option<(Account, Secrets)>,
    /// A renewed sign-in the keychain refused to keep: it plays now, and may have to sign in
    /// again next time.
    pub unsaved: Option<String>,
}

/// The last [`Launch::unsaved`], for the window or the terminal to show once.
static UNSAVED: Mutex<Option<String>> = Mutex::new(None);

/// A Minecraft token for the signed-in `account` with at least [`RENEW_BEFORE`] left: the one
/// the keychain keeps, or one renewed through the refresh token (and kept) when it runs out.
/// For the profile's own changes (its cape), never for the game.
pub fn token_for(
    ep: &Endpoints,
    client_id: Option<&str>,
    store: &dyn SecretStore,
    account: &Account,
) -> Result<String, String> {
    let _operation = session::lock(store)?;
    let secrets = store
        .get(&account.uuid)?
        .and_then(|s| serde_json::from_str::<Secrets>(&s).ok())
        .ok_or_else(|| {
            format!(
                "{} is no longer signed in on this computer. Sign in again.",
                account.name
            )
        })?;
    if secrets.sign_in_required {
        return Err(SIGN_IN_AGAIN.into());
    }
    if secrets.minecraft_expires_at > now() + RENEW_BEFORE {
        return Ok(secrets.minecraft_token);
    }
    let keep = |secrets: &Secrets| {
        serde_json::to_string(secrets)
            .map_err(|e| e.to_string())
            .and_then(|text| store.set(&account.uuid, &text))
    };
    match check(ep, client_id, account, secrets) {
        Check::Owns(_, secrets) => {
            keep(&secrets)?;
            Ok(secrets.minecraft_token)
        }
        Check::DoesNot(secrets) => {
            let _ = keep(&secrets);
            Err(DOES_NOT_OWN.into())
        }
        Check::NoAnswer(e, secrets) => {
            let _ = keep(&secrets);
            Err(e)
        }
    }
}

/// What the last launch couldn't save in the keychain, once.
pub fn take_unsaved() -> Option<String> {
    UNSAVED.lock().ok()?.take()
}

/// The owner gate: the game starts only while an account in the launcher owns Minecraft: Java
/// Edition. The accounts are asked again, the selected one first, until one owns it; each keeps
/// the answer in the keychain. An account that gets no answer (no network) keeps its last one, so
/// being offline never blocks a player who passed before, while "doesn't own" blocks at once.
/// The caller saves `accounts` (renewed names and skins).
pub fn launch(
    ep: &Endpoints,
    client_id: Option<&str>,
    store: &dyn SecretStore,
    accounts: &mut Accounts,
) -> Result<Launch, NoOwner> {
    let selected = accounts.selected.clone();
    let mut order = accounts.accounts.clone();
    order.sort_by_key(|a| Some(&a.uuid) != selected.as_ref());
    let mut pass = Launch::default();
    let mut owned = false;
    let mut unreadable = None;
    for account in order {
        if owned {
            break;
        }
        let plays = Some(&account.uuid) == selected.as_ref();
        let operation = session::lock(store);
        let read = match store.get(&account.uuid) {
            Ok(text) => text.and_then(|s| serde_json::from_str::<Secrets>(&s).ok()),
            Err(why) => {
                // Not "signed out": the keyring didn't say (#12179). With no owner, the refusal
                // says this instead.
                let said = crate::i18n::trf(
                    "Can't read {name}'s sign-in: {why}",
                    &[("name", &account.name), ("why", &why)],
                );
                eprintln!("Sunrise: {said}");
                if plays {
                    pass.offline = Some(said.clone());
                }
                unreadable = Some(said);
                continue;
            }
        };
        let Some(secrets) = read else {
            if plays {
                pass.offline = Some(format!(
                    "{} is no longer signed in on this computer. Sign in again.",
                    account.name
                ));
            }
            continue;
        };
        let before = serde_json::to_string(&secrets).ok();
        let checked = match &operation {
            Ok(_) => check(ep, client_id, &account, secrets),
            Err(e) => Check::NoAnswer(e.clone(), secrets),
        };
        let (said, secrets) = match checked {
            Check::Owns(renewed, secrets) => {
                accounts.update(renewed.clone());
                if plays {
                    pass.session = Some((renewed.clone(), secrets.clone()));
                }
                pass.owner = Some((renewed, secrets.clone()));
                (None, secrets)
            }
            Check::DoesNot(secrets) => (Some(DOES_NOT_OWN.to_string()), secrets),
            Check::NoAnswer(e, secrets) => (Some(e), secrets),
        };
        owned = secrets.owns;
        if plays {
            pass.offline = said;
        }
        let after = serde_json::to_string(&secrets).ok();
        if after != before {
            if let Some(text) = after {
                // The game still starts; the player hears why the next launch may ask again.
                if let Err(e) = store.set(&account.uuid, &text) {
                    let said = format!(
                        "{}'s renewed sign-in couldn't be saved in the keychain ({e}): you may \
                         have to sign in again next time.",
                        account.name
                    );
                    if let Ok(mut last) = UNSAVED.lock() {
                        *last = Some(said.clone());
                    }
                    pass.unsaved = Some(said);
                }
            }
        }
    }
    if owned {
        Ok(pass)
    } else {
        Err(NoOwner {
            keyring: unreadable,
        })
    }
}

/// The owner gate before the game starts: [`launch`], where the account that plays (the selected
/// one, else the first) must own the game itself, by the services' answer or, when they can't be
/// asked, its last one. Never another account's ownership for a name of its own.
pub fn launch_for_game(
    ep: &Endpoints,
    client_id: Option<&str>,
    store: &dyn SecretStore,
    accounts: &mut Accounts,
) -> Result<Launch, NoOwner> {
    if accounts.selected().is_none() {
        accounts.selected = accounts.accounts.first().map(|a| a.uuid.clone());
    }
    let pass = launch(ep, client_id, store, accounts)?;
    let owns = accounts
        .selected()
        .and_then(|a| secrets_in(store, &a.uuid))
        .is_some_and(|s| s.owns);
    if owns {
        Ok(pass)
    } else {
        // Why that account can't play (it doesn't own the game, signed out, an unread keyring).
        Err(NoOwner {
            keyring: pass.offline,
        })
    }
}

/// Write the session for the game, readable by this user only, in a fresh file it deletes
/// when it reads it.
pub fn write_session_file(
    dir: &Path,
    account: &Account,
    secrets: &Secrets,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    // The folder too is this user's only (the files already are).
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700));
    }
    // Leftovers of games that never started (refused by the game after two minutes anyway).
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            let old = e
                .metadata()
                .and_then(|m| m.modified())
                .is_ok_and(|m| m.elapsed().is_ok_and(|age| age > Duration::from_secs(120)));
            if old {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let mut name = [0u8; 12];
    getrandom::fill(&mut name).map_err(|e| e.to_string())?;
    let path = dir.join(format!(
        "session-{}.json",
        name.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let mut file = options.open(&path).map_err(|e| e.to_string())?;
    let body = json!({
        "name": account.name,
        "uuid": account.uuid,
        "access_token": secrets.minecraft_token,
        "user_type": "msa",
        "xuid": secrets.xuid,
    });
    std::io::Write::write_all(&mut file, body.to_string().as_bytes()).map_err(|e| e.to_string())?;
    Ok(path)
}

/// How long a game takes a session file: it refuses one older than this (the game's
/// `MAX_SESSION_FILE_AGE`), so the launcher deletes it then if it is still there.
const SESSION_FILE_LIFE: Duration = Duration::from_secs(120);

/// The session files this launcher wrote for games it starts, until they are gone: a game
/// deletes each as it reads it; the rest go when the game would refuse them anyway
/// ([`SESSION_FILE_LIFE`], a game that never started or crashed first) or when the launch failed
/// ([`discard_unread_sessions`]). Nothing with a token stays on disk.
struct UnreadSession {
    path: PathBuf,
    expire_only: bool,
}

static UNREAD: Mutex<Vec<UnreadSession>> = Mutex::new(Vec::new());

/// [`write_session_file`] for a game this launcher is about to start (see [`UNREAD`]).
fn write_launch_session_file(
    dir: &Path,
    account: &Account,
    secrets: &Secrets,
) -> Result<PathBuf, String> {
    let path = write_session_file(dir, account, secrets)?;
    let Ok(mut unread) = UNREAD.lock() else {
        return Ok(path);
    };
    if unread.is_empty() {
        // One reaper while files wait; it ends with the list (under its lock, so a file added
        // as it ends starts the next).
        let _ = std::thread::Builder::new()
            .name("Session files".into())
            .spawn(|| loop {
                std::thread::sleep(Duration::from_secs(5));
                let Ok(mut unread) = UNREAD.lock() else {
                    return;
                };
                reap(&mut unread, SESSION_FILE_LIFE);
                if unread.is_empty() {
                    return;
                }
            });
    }
    unread.push(UnreadSession {
        path: path.clone(),
        expire_only: false,
    });
    Ok(path)
}

/// Deletes the files of `unread` older than `life` (the game would refuse them) and forgets
/// those gone (read).
fn reap(unread: &mut Vec<UnreadSession>, life: Duration) {
    unread.retain(|file| {
        let waiting = std::fs::metadata(&file.path)
            .and_then(|m| m.modified())
            .is_ok_and(|m| m.elapsed().is_ok_and(|age| age <= life));
        if !waiting {
            let _ = std::fs::remove_file(&file.path);
        }
        waiting
    });
}

/// A managed handoff can be unresolved. Nothing but its own definite failure/exit
/// or the game's usual expiry may delete that launch's file.
pub(crate) fn retain_launch_session(path: Option<&Path>) {
    if let (Some(path), Ok(mut unread)) = (path, UNREAD.lock()) {
        if let Some(file) = unread.iter_mut().find(|file| file.path == path) {
            file.expire_only = true;
        }
    }
}

/// Deletes the session files written for a launch whose game didn't start, or has ended: any it
/// didn't read is left over (a game deletes those it reads).
pub fn discard_unread_sessions() {
    let files: Vec<_> = UNREAD
        .lock()
        .map(|mut unread| unread.extract_if(.., |file| !file.expire_only).collect())
        .unwrap_or_default();
    for file in files {
        let _ = std::fs::remove_file(file.path);
    }
}

/// Discard only the session prepared for the launch that was refused.
pub fn discard_launch_session(file: Option<PathBuf>) {
    if let Some(file) = file {
        if let Ok(mut unread) = UNREAD.lock() {
            unread.retain(|session| session.path != file);
        }
        let _ = std::fs::remove_file(file);
    }
}

/// Called under the account-operation lock, after the final profile is known. Rejected or
/// cancelled sign-ins must not save a credential, add another identity, or change selection.
fn accept_sign_in(
    accounts: &mut Accounts,
    store: &dyn SecretStore,
    expected: Option<&Account>,
    account: Account,
    secrets: &Secrets,
    cancelled: &dyn Fn() -> bool,
    progress: &dyn Fn(session::Phase),
) -> Result<(), String> {
    if cancelled() {
        return Err("Sign-in cancelled.".into());
    }
    if let Some(expected) = expected {
        if expected.uuid != account.uuid {
            return Err(format!(
                "Signed in as {} instead of {}. Use the Microsoft account for {} and try again.",
                account.name, expected.name, expected.name
            ));
        }
        if accounts.find(&expected.uuid).is_none() {
            return Err("That account was signed out while sign-in was in progress. Add it again to continue.".into());
        }
    }
    let selected = accounts.selected.clone();
    progress(session::Phase::Saving);
    accounts.keep_sign_in(store, account, secrets)?;
    if expected.is_some() {
        accounts.selected = selected;
    }
    Ok(())
}

/// Where a background sign-in is (the window and the MCP server show it).
#[derive(Clone, Debug, PartialEq)]
pub enum SignInState {
    Starting,
    /// Waiting for the player at `verification_uri` with `user_code`.
    Code(DeviceCode),
    Working(session::Phase),
    Done(Result<Account, String>),
}

/// A sign-in on its own thread, from the code to the saved account.
pub struct SignIn {
    state: std::sync::Arc<Mutex<SignInState>>,
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl SignIn {
    pub fn start() -> Self {
        Self::start_expected(None)
    }

    /// Restore this exact Minecraft identity without changing who currently plays.
    pub fn start_for(account: Account) -> Self {
        Self::start_expected(Some(account))
    }

    fn start_expected(expected: Option<Account>) -> Self {
        use std::sync::atomic::Ordering;
        let state = std::sync::Arc::new(Mutex::new(SignInState::Starting));
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (thread_state, thread_cancel) = (state.clone(), cancel.clone());
        std::thread::spawn(move || {
            let set = |s| *thread_state.lock().unwrap() = s;
            let result = (|| {
                let ep = Endpoints::live();
                let client_id = client_id().ok_or(NOT_CONFIGURED)?;
                let code = device_code(&ep, &client_id)?;
                set(SignInState::Code(code.clone()));
                let ms = wait_for_sign_in(&ep, &client_id, &code, &|| {
                    thread_cancel.load(Ordering::Relaxed)
                })?;
                let progress = |phase| set(SignInState::Working(phase));
                let (account, secrets) = finish_sign_in_with_progress(&ep, ms, &progress)?;
                progress(session::Phase::Queued);
                let store = store();
                let _operation = session::lock(store)?;
                let mut accounts = Accounts::load();
                accept_sign_in(
                    &mut accounts,
                    store,
                    expected.as_ref(),
                    account.clone(),
                    &secrets,
                    &|| thread_cancel.load(Ordering::Relaxed),
                    &progress,
                )?;
                progress(session::Phase::Saving);
                accounts.save()?;
                Ok(account)
            })();
            set(SignInState::Done(result));
        });
        Self { state, cancel }
    }
    pub fn state(&self) -> SignInState {
        self.state.lock().unwrap().clone()
    }
    pub fn cancel(&self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

/// What the game about to start gets from the accounts.
#[derive(Debug, Default)]
pub struct GameSession {
    /// The playing account's session (`--session-file`); none: offline.
    pub file: Option<PathBuf>,
    /// Why the account plays offline this time, if it does.
    pub offline: Option<String>,
}

impl GameSession {
    /// Gives the game its playing account, or the offline reason shown to the player.
    pub fn apply(self, cmd: &mut std::process::Command) -> Option<String> {
        if let Some(file) = self.file {
            cmd.arg("--session-file").arg(file);
        }
        if let Some(reason) = &self.offline {
            cmd.env("SUNRISE_OFFLINE_REASON", reason);
        }
        self.offline
    }
}

/// The owner gate for the game about to start, then the selected account's session in a file
/// for it.
pub fn session_for_game(game_home: &Path) -> Result<GameSession, NoOwner> {
    let mut accounts = Accounts::load();
    let pass = launch_for_game(
        &Endpoints::live(),
        client_id().as_deref(),
        store(),
        &mut accounts,
    )?;
    if let Err(e) = accounts.save_renewed() {
        eprintln!("Sunrise: the accounts are not saved: {e}");
    }
    Ok(session_file(game_home, pass))
}

/// [`session_for_game`] for the account that was selected when Play was accepted, `accepted`:
/// installing, preparing and building can take minutes, and whoever is selected when they end
/// must not be the one who plays. Which account is selected now is not touched. An account
/// signed out meanwhile doesn't play, and no session file is written for it.
pub fn session_for_game_accepted(game_home: &Path, accepted: &str) -> Result<GameSession, NoOwner> {
    let mut accounts = Accounts::load();
    let pass = launch_accepted(
        &Endpoints::live(),
        client_id().as_deref(),
        store(),
        &mut accounts,
        accepted,
    )?;
    if let Err(e) = accounts.save_renewed() {
        eprintln!("Sunrise: the accounts are not saved: {e}");
    }
    Ok(session_file(game_home, pass))
}

fn launch_accepted(
    ep: &Endpoints,
    client_id: Option<&str>,
    store: &dyn SecretStore,
    accounts: &mut Accounts,
    accepted: &str,
) -> Result<Launch, NoOwner> {
    check_launch_account(accounts, accepted)?;
    let chosen = accounts.selected.replace(accepted.to_owned());
    let pass = launch_for_game(ep, client_id, store, accounts);
    accounts.selected = chosen;
    pass
}

/// The accepted UUID must still be signed in, independent of who is selected now.
pub fn check_launch_account(accounts: &Accounts, accepted: &str) -> Result<(), NoOwner> {
    if !accounts.accounts.iter().any(|a| a.uuid == accepted) {
        return Err(NoOwner {
            keyring: Some(
                crate::i18n::tr(
                    "The account Play was pressed for was signed out before the game started. Press Play again.",
                )
                .into(),
            ),
        });
    }
    Ok(())
}

/// [`session_for_game`] for one launch as `who` (`play --account`, a signed-in account): who
/// plays changes for this launch only, so several games can play as several accounts at once
/// (#4952). What the services renewed (tokens, names, skins) is still kept; which account
/// `account select` picked is not touched.
pub fn session_for_game_as(game_home: &Path, who: &str) -> Result<GameSession, String> {
    let mut accounts = Accounts::load();
    let pass = launch_as(
        &Endpoints::live(),
        client_id().as_deref(),
        store(),
        &mut accounts,
        who,
    );
    if let Err(e) = accounts.save_renewed() {
        eprintln!("Sunrise: the accounts are not saved: {e}");
    }
    Ok(session_file(game_home, pass?))
}

/// The owner gate as `who`, on `accounts`, which keeps its choice of player.
fn launch_as(
    ep: &Endpoints,
    client_id: Option<&str>,
    store: &dyn SecretStore,
    accounts: &mut Accounts,
    who: &str,
) -> Result<Launch, String> {
    let chosen = accounts.selected.clone();
    let pass = accounts
        .play_as(who)
        .and_then(|()| launch_for_game(ep, client_id, store, accounts).map_err(|e| e.to_string()));
    accounts.selected = chosen;
    pass
}

/// The game's session files for a launch that passed the gate.
fn session_file(game_home: &Path, pass: Launch) -> GameSession {
    let mut out = GameSession {
        offline: pass.offline,
        ..GameSession::default()
    };
    if let Some((account, secrets)) = pass.session {
        match write_launch_session_file(&game_home.join("sessions"), &account, &secrets) {
            Ok(file) => out.file = Some(file),
            Err(e) => out.offline = Some(e),
        }
    }
    out
}

#[cfg(test)]
mod tests;
