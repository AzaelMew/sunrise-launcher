//! The blocky look's Settings page, the native build consent and the settings rows.
use super::*;

impl Launcher {
    pub(crate) fn request_native_build_consent(&mut self) {
        self.native_consent_build = None;
        // Play can ask before Mods has ever opened. Bind the same folder for the names in
        // the question, Decline's toggle and the build that Allow dispatches.
        let selected = self.selected_instance().map(|i| i.home.clone());
        self.pending_consent = selected.map(|home| self.mods.build_consent(home));
    }

    /// Cancel and Escape share the transition; neither can execute a pending action.
    pub(crate) fn cancel_native_consent(&mut self) {
        self.play_after_build = None;
        self.native_consent_build = None;
        if let Err(error) = native_consent::resolve(
            &mut self.pending_consent,
            false,
            true,
            &mut self.settings,
            &self.mods.home(),
        ) {
            self.mods.message = Some(Err(error));
        }
        self.consent_error = None;
        self.instance_mods_changed();
    }

    /// Keeps the Proxy field in `settings.json`, and tells the parts that build their own agents
    /// (`sunrise-port-pack`) which proxy to use.
    pub(crate) fn save_proxy(&mut self) {
        let value = self.proxy_input.text().trim().to_owned();
        self.settings.proxy = (!value.is_empty()).then_some(value);
        self.save_settings();
        sunrise_launcher::proxy::announce(self.settings.proxy.clone());
    }
}
