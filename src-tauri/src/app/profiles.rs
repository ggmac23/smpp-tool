//! Saved connections, and the secrets they deliberately do not contain.
//!
//! A profile is a JSON record on disk. Passwords and TLS key passphrases go
//! to the system keychain instead, addressed by profile id, and the `Profile`
//! type has nowhere to put one: the guarantee that no secret reaches the file
//! is the shape of the type, not the care of the caller.
//!
//! TLS material is held as **paths**. The files are read at connect time, so
//! a renewed certificate takes effect without re-saving the profile.

use std::path::{Path, PathBuf};

use crate::app::dlr::CorrelationConfig;
use crate::app::session::BindMode;

const PROFILES_FILE: &str = "profiles.json";
const KEYCHAIN_SERVICE: &str = "smppclient";

/// The bind fields worth saving. `password` is absent on purpose.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileBind {
    pub system_id: String,
    pub system_type: String,
    pub interface_version: u8,
    pub mode: BindMode,
    #[serde(default)]
    pub addr_ton: u8,
    #[serde(default)]
    pub addr_npi: u8,
    #[serde(default)]
    pub address_range: String,
}

/// The TLS fields worth saving: switches and paths, never contents and never
/// the key passphrase.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileTls {
    pub enabled: bool,
    pub reject_unauthorized: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub servername: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ca_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_cert_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_key_path: Option<String>,
}

/// A saved connection. Deserialising ignores fields it does not know, so a
/// caller that hands this type a blob with a password in it loses the
/// password rather than writing it to disk.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub bind: ProfileBind,
    pub tls: ProfileTls,
    #[serde(default)]
    pub correlation: CorrelationConfig,
}

/// The seam over the system keychain, so the store's logic is testable
/// without one.
pub trait SecretStore: Send + Sync {
    /// Whether this machine can hold a secret at all. A locked or absent
    /// keychain means the store refuses to save one.
    fn available(&self) -> bool;
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

/// The real store: one keychain item per secret, named `<id>/password` or
/// `<id>/key-passphrase` under the `smppclient` service.
pub struct KeyringSecrets;

impl KeyringSecrets {
    fn entry(key: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(KEYCHAIN_SERVICE, key)
            .map_err(|e| format!("the keychain refused the entry {key}: {e}"))
    }
}

impl SecretStore for KeyringSecrets {
    fn available(&self) -> bool {
        // You cannot know a keychain will take a secret without asking it to,
        // and a probe entry would leave a trace in the user's keychain and may
        // prompt them. So this says yes and `set` carries the refusal; `save`
        // adds the advice either way.
        true
    }

    fn get(&self, key: &str) -> Option<String> {
        Self::entry(key).ok()?.get_password().ok()
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        Self::entry(key)?
            .set_password(value)
            .map_err(|e| format!("the keychain would not store the secret: {e}"))
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) => Ok(()),
            // Nothing stored under that name is the state we wanted anyway.
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(format!("the keychain would not drop the secret: {e}")),
        }
    }
}

/// Profiles as JSON in `dir`, secrets in the keychain beside them.
pub struct ProfileStore {
    dir: PathBuf,
    secrets: Box<dyn SecretStore>,
}

impl ProfileStore {
    pub fn new(dir: impl AsRef<Path>, secrets: Box<dyn SecretStore>) -> Self {
        Self {
            dir: dir.as_ref().to_path_buf(),
            secrets,
        }
    }

    pub fn list(&self) -> Vec<Profile> {
        self.read_profiles()
    }

    /// Saves `p`, and any secret given with it. A secret the keychain cannot
    /// hold is refused outright — writing it in the clear would be worse than
    /// not saving at all — and nothing is written in that case. Passing
    /// `None` keeps whatever is already stored, so editing a profile does not
    /// silently drop its password.
    pub fn save(
        &self,
        p: &Profile,
        password: Option<&str>,
        key_passphrase: Option<&str>,
    ) -> Result<(), String> {
        let has_secret = password.is_some() || key_passphrase.is_some();
        if has_secret && !self.secrets.available() {
            return Err(advise("this machine's keychain is unavailable"));
        }

        // Secrets first: a failure there must not leave a profile referring
        // to a password that was never stored.
        if let Some(pw) = password {
            self.secrets.set(&password_key(&p.id), pw).map_err(advise)?;
        }
        if let Some(pass) = key_passphrase {
            self.secrets
                .set(&passphrase_key(&p.id), pass)
                .map_err(advise)?;
        }

        let mut profiles = self.read_profiles();
        match profiles.iter().position(|x| x.id == p.id) {
            Some(i) => profiles[i] = p.clone(),
            None => profiles.push(p.clone()),
        }
        self.write_profiles(&profiles)
    }

    pub fn password(&self, id: &str) -> Option<String> {
        self.secrets.get(&password_key(id))
    }

    pub fn key_passphrase(&self, id: &str) -> Option<String> {
        self.secrets.get(&passphrase_key(id))
    }

    pub fn remove(&self, id: &str) -> Result<(), String> {
        let kept: Vec<Profile> = self
            .read_profiles()
            .into_iter()
            .filter(|p| p.id != id)
            .collect();
        self.write_profiles(&kept)?;
        self.secrets.delete(&password_key(id))?;
        self.secrets.delete(&passphrase_key(id))
    }

    /* ------------------------------------------------------------------ */

    fn read_profiles(&self) -> Vec<Profile> {
        // Missing or unreadable: start empty rather than refuse to open. A
        // corrupt file costs the saved profiles, not the use of the program.
        std::fs::read_to_string(self.dir.join(PROFILES_FILE))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn write_profiles(&self, profiles: &[Profile]) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("could not create {}: {e}", self.dir.display()))?;
        let json = serde_json::to_string_pretty(profiles)
            .map_err(|e| format!("could not serialise the profiles: {e}"))?;
        std::fs::write(self.dir.join(PROFILES_FILE), json)
            .map_err(|e| format!("could not write the profiles: {e}"))
    }
}

/// Keeps the cause and adds the way out, so a refused save tells the user
/// what to do rather than only what went wrong.
fn advise(cause: impl std::fmt::Display) -> String {
    format!(
        "{cause}, so the secret cannot be stored; save the profile without one \
         and type it at bind time"
    )
}

fn password_key(id: &str) -> String {
    format!("{id}/password")
}

fn passphrase_key(id: &str) -> String {
    format!("{id}/key-passphrase")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::dlr::CorrelationConfig;
    use crate::app::session::BindMode;
    use crate::testing::tmp::TempDir;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// A stand-in for the system keychain, so the store's logic is testable
    /// without one. `works: false` is a machine whose keychain is locked or
    /// absent — the case where plaintext must never be the fallback.
    struct FakeSecrets {
        works: bool,
        items: Mutex<HashMap<String, String>>,
    }

    impl FakeSecrets {
        fn new() -> Self {
            Self {
                works: true,
                items: Mutex::new(HashMap::new()),
            }
        }
        fn broken() -> Self {
            Self {
                works: false,
                items: Mutex::new(HashMap::new()),
            }
        }
        fn len(&self) -> usize {
            self.items.lock().unwrap().len()
        }
    }

    impl SecretStore for FakeSecrets {
        fn available(&self) -> bool {
            self.works
        }
        fn get(&self, key: &str) -> Option<String> {
            self.items.lock().unwrap().get(key).cloned()
        }
        fn set(&self, key: &str, value: &str) -> Result<(), String> {
            if !self.works {
                return Err("keychain unavailable".into());
            }
            self.items
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), String> {
            self.items.lock().unwrap().remove(key);
            Ok(())
        }
    }

    fn profile() -> Profile {
        Profile {
            id: "p1".into(),
            name: "Aggregator A".into(),
            host: "smsc.example.com".into(),
            port: 2775,
            bind: ProfileBind {
                system_id: "esme".into(),
                system_type: "SMPP".into(),
                interface_version: 0x34,
                mode: BindMode::Transceiver,
                addr_ton: 0,
                addr_npi: 0,
                address_range: String::new(),
            },
            tls: ProfileTls {
                enabled: true,
                reject_unauthorized: false,
                servername: Some("smsc.example.com".into()),
                ca_path: None,
                client_cert_path: None,
                client_key_path: None,
            },
            correlation: CorrelationConfig::default(),
        }
    }

    fn store(dir: &TempDir) -> ProfileStore {
        ProfileStore::new(dir.path(), Box::new(FakeSecrets::new()))
    }

    #[test]
    fn reads_the_profile_the_renderer_sends_without_losing_the_tls_paths() {
        // This is `buildProfile`'s output, verbatim. The widget used to put
        // the three paths beside `tls` rather than inside it; serde dropped
        // them without a word, and a profile saved with a CA and a client
        // certificate came back with neither — so the bind failed on the
        // handshake and nothing said why.
        let sent = serde_json::json!({
            "id": "p1",
            "name": "Staging",
            "host": "smsc.example.com",
            "port": 2775,
            "bind": {
                "systemId": "esme",
                "systemType": "SMPP",
                "interfaceVersion": 0x34,
                "mode": "transceiver",
                "addrTon": 0,
                "addrNpi": 0,
                "addressRange": "^4477.*"
            },
            "tls": {
                "enabled": true,
                "rejectUnauthorized": false,
                "servername": "smsc.example.com",
                "caPath": "/etc/ssl/ca.pem",
                "clientCertPath": "/etc/ssl/client.crt",
                "clientKeyPath": "/etc/ssl/client.key"
            },
            "correlation": {
                "preferTlv": true,
                "parseText": true,
                "normaliseRadix": true,
                "stripLeadingZeros": true
            }
        });

        let profile: Profile = serde_json::from_value(sent).expect("the renderer's shape");
        assert_eq!(profile.tls.ca_path.as_deref(), Some("/etc/ssl/ca.pem"));
        assert_eq!(
            profile.tls.client_cert_path.as_deref(),
            Some("/etc/ssl/client.crt")
        );
        assert_eq!(
            profile.tls.client_key_path.as_deref(),
            Some("/etc/ssl/client.key")
        );
        assert_eq!(profile.bind.address_range, "^4477.*");
        assert_eq!(profile.bind.system_type, "SMPP");
    }

    #[test]
    fn a_saved_profile_comes_back_with_its_tls_paths() {
        let dir = TempDir::new("profiles-tlspaths");
        let s = store(&dir);
        let mut p = profile();
        p.tls.ca_path = Some("/etc/ssl/ca.pem".into());
        p.tls.client_cert_path = Some("/etc/ssl/client.crt".into());
        p.tls.client_key_path = Some("/etc/ssl/client.key".into());
        p.bind.address_range = "^4477.*".into();

        s.save(&p, None, None).unwrap();
        let back = &store(&dir).list()[0];
        assert_eq!(back.tls.ca_path.as_deref(), Some("/etc/ssl/ca.pem"));
        assert_eq!(
            back.tls.client_key_path.as_deref(),
            Some("/etc/ssl/client.key")
        );
        assert_eq!(back.bind.address_range, "^4477.*");
    }

    #[test]
    fn returns_an_empty_list_for_a_fresh_directory() {
        let dir = TempDir::new("profiles-empty");
        assert_eq!(store(&dir).list(), vec![]);
    }

    #[test]
    fn round_trips_a_profile_through_save_and_list() {
        let dir = TempDir::new("profiles-roundtrip");
        store(&dir).save(&profile(), Some("hunter2"), None).unwrap();
        assert_eq!(store(&dir).list(), vec![profile()]);
    }

    #[test]
    fn never_writes_the_password_into_the_profile_file() {
        let dir = TempDir::new("profiles-nopass");
        store(&dir).save(&profile(), Some("hunter2"), None).unwrap();
        let raw = std::fs::read_to_string(dir.join("profiles.json")).unwrap();
        assert!(
            !raw.contains("hunter2"),
            "the profile file held the password: {raw}"
        );
    }

    #[test]
    fn returns_the_password_through_the_secret_store() {
        let dir = TempDir::new("profiles-readpass");
        // One keychain, two stores: what the first wrote the second must read,
        // which is what reopening the app does.
        let shared = std::sync::Arc::new(FakeSecrets::new());
        let s = ProfileStore::new(dir.path(), Box::new(SharedSecrets(shared.clone())));
        s.save(&profile(), Some("hunter2"), None).unwrap();
        let reopened = ProfileStore::new(dir.path(), Box::new(SharedSecrets(shared)));
        assert_eq!(reopened.password("p1").as_deref(), Some("hunter2"));
    }

    /// Lets two `ProfileStore`s share one fake keychain, the way they share
    /// the real one.
    struct SharedSecrets(std::sync::Arc<FakeSecrets>);

    impl SecretStore for SharedSecrets {
        fn available(&self) -> bool {
            self.0.available()
        }
        fn get(&self, key: &str) -> Option<String> {
            self.0.get(key)
        }
        fn set(&self, key: &str, value: &str) -> Result<(), String> {
            self.0.set(key, value)
        }
        fn delete(&self, key: &str) -> Result<(), String> {
            self.0.delete(key)
        }
    }

    #[test]
    fn stores_a_tls_key_passphrase_separately_from_the_password() {
        let dir = TempDir::new("profiles-keypass");
        let s = store(&dir);
        s.save(&profile(), Some("hunter2"), Some("keypass"))
            .unwrap();
        assert_eq!(s.password("p1").as_deref(), Some("hunter2"));
        assert_eq!(s.key_passphrase("p1").as_deref(), Some("keypass"));
    }

    #[test]
    fn never_writes_the_key_passphrase_into_the_profile_file() {
        let dir = TempDir::new("profiles-nokeypass");
        store(&dir)
            .save(&profile(), Some("hunter2"), Some("keypass"))
            .unwrap();
        let raw = std::fs::read_to_string(dir.join("profiles.json")).unwrap();
        assert!(!raw.contains("keypass"));
    }

    #[test]
    fn refuses_rather_than_storing_a_password_the_keychain_cannot_hold() {
        let dir = TempDir::new("profiles-broken");
        let s = ProfileStore::new(dir.path(), Box::new(FakeSecrets::broken()));
        let err = s.save(&profile(), Some("hunter2"), None).unwrap_err();
        assert!(
            err.to_lowercase().contains("keychain"),
            "the refusal should name the keychain: {err}"
        );
        assert!(
            !dir.join("profiles.json").exists(),
            "a refused save must not leave a profile behind"
        );
    }

    #[test]
    fn saves_a_profile_with_no_password_even_without_a_keychain() {
        let dir = TempDir::new("profiles-nosecret");
        let s = ProfileStore::new(dir.path(), Box::new(FakeSecrets::broken()));
        s.save(&profile(), None, None).unwrap();
        assert_eq!(s.list(), vec![profile()]);
        assert_eq!(s.password("p1"), None);
    }

    #[test]
    fn returns_none_for_the_password_of_an_unknown_profile() {
        let dir = TempDir::new("profiles-unknown");
        assert_eq!(store(&dir).password("nope"), None);
    }

    #[test]
    fn replaces_a_profile_saved_under_the_same_id() {
        let dir = TempDir::new("profiles-replace");
        let s = store(&dir);
        s.save(&profile(), Some("a"), None).unwrap();
        let mut renamed = profile();
        renamed.name = "Renamed".into();
        s.save(&renamed, Some("b"), None).unwrap();
        let list = s.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Renamed");
        assert_eq!(s.password("p1").as_deref(), Some("b"));
    }

    #[test]
    fn removes_a_profile_and_its_secrets() {
        let dir = TempDir::new("profiles-remove");
        let secrets = std::sync::Arc::new(FakeSecrets::new());
        let s = ProfileStore::new(dir.path(), Box::new(SharedSecrets(secrets.clone())));
        s.save(&profile(), Some("hunter2"), Some("keypass"))
            .unwrap();
        assert_eq!(secrets.len(), 2);
        s.remove("p1").unwrap();
        assert_eq!(s.list(), vec![]);
        assert_eq!(s.password("p1"), None);
        assert_eq!(secrets.len(), 0, "the secrets outlived the profile");
    }

    #[test]
    fn keeps_several_profiles_apart() {
        let dir = TempDir::new("profiles-several");
        let s = store(&dir);
        s.save(&profile(), Some("a"), None).unwrap();
        let mut second = profile();
        second.id = "p2".into();
        second.name = "Staging".into();
        s.save(&second, Some("b"), None).unwrap();
        assert_eq!(
            s.list().iter().map(|p| p.id.clone()).collect::<Vec<_>>(),
            ["p1", "p2"]
        );
        assert_eq!(s.password("p1").as_deref(), Some("a"));
        assert_eq!(s.password("p2").as_deref(), Some("b"));
    }

    #[test]
    fn survives_a_corrupt_profile_file_by_starting_empty() {
        let dir = TempDir::new("profiles-corrupt");
        std::fs::write(dir.join("profiles.json"), "{ not json").unwrap();
        assert_eq!(store(&dir).list(), vec![]);
    }

    #[test]
    fn keeps_the_password_when_a_profile_is_saved_again_without_one() {
        let dir = TempDir::new("profiles-keeppass");
        let s = store(&dir);
        s.save(&profile(), Some("hunter2"), None).unwrap();
        let mut edited = profile();
        edited.name = "Edited".into();
        s.save(&edited, None, None).unwrap();
        assert_eq!(s.password("p1").as_deref(), Some("hunter2"));
    }

    /// A keychain that says yes and then fails, which is what a real one does
    /// — you cannot know it will refuse until you ask it to store something.
    struct LyingSecrets;

    impl SecretStore for LyingSecrets {
        fn available(&self) -> bool {
            true
        }
        fn get(&self, _key: &str) -> Option<String> {
            None
        }
        fn set(&self, _key: &str, _value: &str) -> Result<(), String> {
            Err("the keychain is locked".into())
        }
        fn delete(&self, _key: &str) -> Result<(), String> {
            Ok(())
        }
    }

    #[test]
    fn a_keychain_that_fails_on_write_refuses_the_save_and_says_what_to_do() {
        let dir = TempDir::new("profiles-lying");
        let s = ProfileStore::new(dir.path(), Box::new(LyingSecrets));
        let err = s.save(&profile(), Some("hunter2"), None).unwrap_err();
        assert!(
            err.contains("the keychain is locked"),
            "the cause was lost: {err}"
        );
        assert!(
            err.contains("bind time"),
            "the refusal should say what to do instead: {err}"
        );
        assert!(
            !dir.join("profiles.json").exists(),
            "a refused save must not leave a profile behind"
        );
    }

    #[test]
    fn writes_only_known_fields_even_if_handed_extra_ones() {
        // A caller that hands the store a profile-shaped blob with a password
        // in it must not get that password written to disk. The guarantee is
        // the type, not the caller's good manners.
        let handed = serde_json::json!({
            "id": "p1",
            "name": "Aggregator A",
            "host": "smsc.example.com",
            "port": 2775,
            "password": "hunter2",
            "bind": {
                "systemId": "esme",
                "password": "hunter2",
                "systemType": "SMPP",
                "interfaceVersion": 0x34,
                "mode": "transceiver",
                "addrTon": 0,
                "addrNpi": 0,
                "addressRange": ""
            },
            "tls": {
                "enabled": true,
                "rejectUnauthorized": false,
                "servername": "smsc.example.com",
                "clientKeyPassphrase": "keypass",
                "ca": "-----BEGIN CERTIFICATE-----"
            },
            "correlation": {
                "preferTlv": true,
                "parseText": true,
                "normaliseRadix": true,
                "stripLeadingZeros": true
            }
        });

        let dir = TempDir::new("profiles-whitelist");
        let parsed: Profile = serde_json::from_value(handed).unwrap();
        store(&dir).save(&parsed, None, None).unwrap();

        let raw = std::fs::read_to_string(dir.join("profiles.json")).unwrap();
        assert!(
            !raw.contains("hunter2"),
            "a password reached the profile file: {raw}"
        );
        assert!(
            !raw.contains("keypass"),
            "a key passphrase reached the profile file: {raw}"
        );
        assert!(
            !raw.contains("BEGIN CERTIFICATE"),
            "a key reached the profile file: {raw}"
        );
        assert!(
            raw.contains("Aggregator A"),
            "the profile itself was not written: {raw}"
        );
    }
}
