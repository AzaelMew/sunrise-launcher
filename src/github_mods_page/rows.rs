//! Row layout helpers for the "Add from source" sheet.
/// A 920 px window stacks the two source cards; a narrower one keeps them side by side.
pub(super) fn is_narrow(width: f32) -> bool {
    width < 920.
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_source_cards_stack_below_the_wide_layout() {
        assert!(is_narrow(919.));
        assert!(!is_narrow(920.));
    }
}
