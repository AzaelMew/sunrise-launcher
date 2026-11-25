//! Saving the Proxy field of Settings.
use super::*;

impl Launcher {
    /// Keeps the Proxy field in `settings.json`, and tells the parts that build their own agents
    /// (`sunrise-port-pack`) which proxy to use.
    pub(crate) fn save_proxy(&mut self) {
        let value = self.proxy_input.text().trim().to_owned();
        self.settings.proxy = (!value.is_empty()).then_some(value);
        self.save_settings();
        sunrise_launcher::proxy::announce(self.settings.proxy.clone());
    }
}
