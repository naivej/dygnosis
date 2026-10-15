//! Accepted opener options from the shared pinned syntax record owner.
//! Body instructions and user-defined partition keys remain separate forms.

pub fn allows(command: &str, option: &str) -> Option<bool> {
    crate::engine::catalog::pinned_option_names(command)
        .map(|names| names.iter().any(|name| name.eq_ignore_ascii_case(option)))
}
