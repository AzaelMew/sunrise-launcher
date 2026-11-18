//! The Create a server dialog in the glass look (the mockup's `ServerModal`, sunrise-site
//! `components/launcher/server-modal.tsx`): one plate round at 28 over the dimmed page. On the
//! left, what the server will be (a picture, its name and its rules); on the right, its name,
//! world, game mode, difficulty, how many players, who may join and whether accounts are
//! checked; the EULA, Cancel and Create at the foot. Dark appearance only.
use super::*;






/// A player name Minecraft accepts: 3 to 16 letters, digits or underscores.
fn valid_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl Launcher {

    /// The name typed in the friend field joins the list, if Minecraft would take it.
    pub(super) fn add_create_friend(&mut self) {
        let typed = self.servers.value(Field::Players).trim().to_string();
        let Some(c) = &mut self.servers.create else {
            return;
        };
        if valid_name(&typed) && !c.friends.contains(&typed) {
            c.friends.push(typed);
            self.servers.text(Field::Players).set(String::new());
        }
    }

}

#[cfg(test)]
mod tests {
    use super::valid_name;

    #[test]
    fn friend_names_follow_minecrafts_rules() {
        assert!(valid_name("Player46008"));
        assert!(valid_name("oak_leaf"));
        assert!(!valid_name("ab"));
        assert!(!valid_name("seventeen_letters"));
        assert!(!valid_name("no spaces"));
        assert!(!valid_name("dash-name"));
    }
}
