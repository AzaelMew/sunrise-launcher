//! The standalone launcher's in-process setting and reqwest adapter.

use super::{KEPT, ProxyError, bypasses_proxy, env_proxy, no_proxy, parse_proxy};
use std::ffi::OsString;
use url::Url;

pub(super) type Selected = Option<(&'static str, String)>;
const LAUNCHER_SETTING: &str = "Launcher proxy setting";

/// Check a nonblank address before saving it, without echoing it in an error.
pub fn validate(spec: &str) -> Result<(), ProxyError> {
    parse_proxy(spec).map(|_| ()).map_err(|kind| ProxyError {
        setting: LAUNCHER_SETTING,
        kind,
    })
}

/// Select the route for the launcher's streaming HTTP client. That client parses the address
/// itself because its transport supports more proxy schemes than ureq.
pub fn address_for(url: &Url, setting: Option<&str>) -> Result<Option<String>, ProxyError> {
    select(Some(url), setting, &no_proxy(), |name| {
        std::env::var_os(name)
    })
    .map(|route| route.map(|(_, spec)| spec))
}

pub(super) fn select(
    url: Option<&Url>,
    setting: Option<&str>,
    no_proxy: &str,
    read: impl FnMut(&str) -> Option<OsString>,
) -> Result<Selected, ProxyError> {
    if url.is_some_and(|url| bypasses_proxy(url, no_proxy)) {
        return Ok(None);
    }
    if let Some(spec) = setting {
        return Ok(Some((LAUNCHER_SETTING, spec.to_owned())));
    }
    env_proxy(url.map_or("https", Url::scheme), read)
}

/// Drop cached routes when the launcher changes its configured proxy.
pub fn clear_agents() {
    KEPT.lock().unwrap_or_else(|e| e.into_inner()).clear();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_setting_precedes_environment_but_not_bypass() {
        let url = Url::parse("https://downloads.example:8443/file").unwrap();
        let spec = "socks5h://configured.example:1080";
        let read = |_: &str| Some(OsString::from("http://env.example:3128"));
        let route = |setting, no_proxy| select(Some(&url), setting, no_proxy, read).unwrap();
        assert_eq!(route(Some(spec), ""), Some((LAUNCHER_SETTING, spec.into())));
        assert_eq!(route(Some(spec), "downloads.example:8443"), None);
        assert_eq!(
            route(None, ""),
            Some(("HTTPS_PROXY", "http://env.example:3128".into()))
        );
    }
}
