//! Server passwords in the OS credential store (Windows Credential Manager, macOS Keychain).
//!
//! Elsewhere `available()` is false and passwords stay in the config file as before.

const SERVICE: &str = "djcast";

/// Account name of the Discord webhook URL.
pub const DISCORD_WEBHOOK: &str = "share:discord-webhook";

fn account(profile: &str) -> String {
    format!("server-profile:{profile}")
}

/// Whether passwords can be kept in the OS credential store on this platform.
pub fn available() -> bool {
    cfg!(any(target_os = "windows", target_os = "macos"))
}

pub fn set(profile: &str, password: &str) -> keyring::Result<()> {
    set_named(&account(profile), password)
}

pub fn set_named(name: &str, value: &str) -> keyring::Result<()> {
    keyring::Entry::new(SERVICE, name)?.set_password(value)
}

pub fn get_named(name: &str) -> Option<String> {
    if !available() {
        return None;
    }
    keyring::Entry::new(SERVICE, name).and_then(|e| e.get_password()).ok()
}

pub fn delete_named(name: &str) {
    if available() {
        let _ = keyring::Entry::new(SERVICE, name).and_then(|e| e.delete_credential());
    }
}

/// The stored password, or `None` if there is none (or the store is unavailable).
pub fn get(profile: &str) -> Option<String> {
    if !available() {
        return None;
    }
    match keyring::Entry::new(SERVICE, &account(profile)).and_then(|e| e.get_password()) {
        Ok(p) => Some(p),
        Err(keyring::Error::NoEntry) => None,
        Err(e) => {
            log::warn!("reading the password of \"{profile}\" from the credential store: {e}");
            None
        }
    }
}

pub fn delete(profile: &str) {
    if !available() {
        return;
    }
    match keyring::Entry::new(SERVICE, &account(profile)).and_then(|e| e.delete_credential()) {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => log::warn!("removing the password of \"{profile}\" from the credential store: {e}"),
    }
}
