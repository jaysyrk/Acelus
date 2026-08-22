use acelus_auth::store::{
    AccountStore, Error, FileSecrets, MemorySecrets, SecretStore, StoredAccount, REFRESH_TOKEN_KEY,
    SESSION_TOKEN_KEY,
};
use acelus_auth::{Account, Secret};

const NOW: u64 = 1_700_000_000;

fn account(uuid: &str, name: &str) -> Account {
    Account {
        uuid: uuid.into(),
        name: name.into(),
        minecraft_token: Secret::new(format!("{name}-session-token")),
        refresh_token: Some(Secret::new(format!("{name}-refresh-token"))),
        expires_in: 86400,
        xuid: Some("2535416239104446".into()),
        entitlement_verified: true,
        skin_url: Some("https://textures/skin".into()),
        cape_url: None,
    }
}

fn store(dir: &tempfile::TempDir) -> AccountStore {
    AccountStore::new(
        dir.path().join("state/accounts.json"),
        Box::new(MemorySecrets::new()),
    )
}

#[test]
fn an_empty_registry_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let registry = store(&dir).load().unwrap();

    assert!(registry.accounts.is_empty());
    assert!(registry.active.is_none());
    assert!(registry.active_account().is_none());
}

#[test]
fn remembering_an_account_persists_it_and_makes_it_active() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);

    let stored = store.remember(&account("uuid-a", "Alex"), NOW).unwrap();
    assert_eq!(stored.expires_at, NOW + 86400);

    let registry = store.load().unwrap();
    assert_eq!(registry.accounts.len(), 1);
    assert_eq!(registry.active.as_deref(), Some("uuid-a"));
    assert_eq!(registry.active_account().unwrap().name, "Alex");
}

#[test]
fn credentials_go_to_the_secret_store_and_never_into_the_registry_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    store.remember(&account("uuid-a", "Alex"), NOW).unwrap();

    let on_disk = std::fs::read_to_string(store.path()).unwrap();
    assert!(
        !on_disk.contains("Alex-refresh-token"),
        "the refresh token must not be written to the registry file"
    );
    assert!(
        !on_disk.contains("Alex-session-token"),
        "the session token must not be written to the registry file"
    );
    assert!(on_disk.contains("Alex"));

    assert_eq!(
        store.refresh_token("uuid-a").unwrap().unwrap().expose(),
        "Alex-refresh-token"
    );
    assert_eq!(
        store.session_token("uuid-a").unwrap().unwrap().expose(),
        "Alex-session-token"
    );
}

#[test]
fn a_second_account_does_not_displace_the_active_one() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);

    store.remember(&account("uuid-a", "Alex"), NOW).unwrap();
    store.remember(&account("uuid-b", "Steve"), NOW).unwrap();

    let registry = store.load().unwrap();
    assert_eq!(registry.accounts.len(), 2);
    assert_eq!(
        registry.active.as_deref(),
        Some("uuid-a"),
        "signing in a second account should not silently switch the active one"
    );
}

#[test]
fn signing_in_again_replaces_rather_than_duplicates() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);

    store.remember(&account("uuid-a", "Alex"), NOW).unwrap();

    let mut renamed = account("uuid-a", "Alex");
    renamed.name = "AlexRenamed".into();
    renamed.minecraft_token = Secret::new("fresh-session-token");
    store.remember(&renamed, NOW + 100).unwrap();

    let registry = store.load().unwrap();
    assert_eq!(registry.accounts.len(), 1);
    assert_eq!(registry.accounts[0].name, "AlexRenamed");
    assert_eq!(registry.accounts[0].expires_at, NOW + 100 + 86400);
    assert_eq!(
        store.session_token("uuid-a").unwrap().unwrap().expose(),
        "fresh-session-token"
    );
}

#[test]
fn the_active_account_can_be_switched() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);

    store.remember(&account("uuid-a", "Alex"), NOW).unwrap();
    store.remember(&account("uuid-b", "Steve"), NOW).unwrap();

    store.set_active("uuid-b").unwrap();
    assert_eq!(store.load().unwrap().active.as_deref(), Some("uuid-b"));
}

#[test]
fn switching_to_an_unknown_account_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    store.remember(&account("uuid-a", "Alex"), NOW).unwrap();

    assert!(matches!(
        store.set_active("uuid-does-not-exist"),
        Err(Error::UnknownAccount { .. })
    ));
}

#[test]
fn forgetting_an_account_erases_its_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    store.remember(&account("uuid-a", "Alex"), NOW).unwrap();

    store.forget("uuid-a").unwrap();

    assert!(store.load().unwrap().accounts.is_empty());
    assert_eq!(
        store.refresh_token("uuid-a").unwrap(),
        None,
        "forgetting an account must erase its refresh token, not just hide it"
    );
    assert_eq!(store.session_token("uuid-a").unwrap(), None);
}

#[test]
fn forgetting_the_active_account_promotes_another() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);

    store.remember(&account("uuid-a", "Alex"), NOW).unwrap();
    store.remember(&account("uuid-b", "Steve"), NOW).unwrap();
    store.set_active("uuid-a").unwrap();

    store.forget("uuid-a").unwrap();

    let registry = store.load().unwrap();
    assert_eq!(registry.active.as_deref(), Some("uuid-b"));
}

#[test]
fn forgetting_the_last_account_leaves_nothing_active() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    store.remember(&account("uuid-a", "Alex"), NOW).unwrap();

    store.forget("uuid-a").unwrap();

    assert!(store.load().unwrap().active.is_none());
}

#[test]
fn forgetting_an_unknown_account_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    assert!(matches!(
        store(&dir).forget("nobody"),
        Err(Error::UnknownAccount { .. })
    ));
}

#[test]
fn a_corrupt_registry_is_reported_rather_than_silently_reset() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    std::fs::create_dir_all(store.path().parent().unwrap()).unwrap();
    std::fs::write(store.path(), b"{not json").unwrap();

    assert!(
        matches!(store.load(), Err(Error::Corrupt { .. })),
        "silently discarding a damaged registry would lose every signed in account"
    );
}

#[test]
fn stored_accounts_survive_a_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = MemorySecrets::new();
    secrets
        .set("uuid-a:refresh", &Secret::new("persisted"))
        .unwrap();

    let path = dir.path().join("state/accounts.json");
    AccountStore::new(path.clone(), Box::new(MemorySecrets::new()))
        .remember(&account("uuid-a", "Alex"), NOW)
        .unwrap();

    let reopened = AccountStore::new(path, Box::new(secrets));
    let registry = reopened.load().unwrap();

    assert_eq!(registry.accounts.len(), 1);
    assert_eq!(
        reopened.refresh_token("uuid-a").unwrap().unwrap().expose(),
        "persisted"
    );
}

#[test]
fn the_secret_key_names_are_stable() {
    assert_eq!(REFRESH_TOKEN_KEY, "refresh");
    assert_eq!(SESSION_TOKEN_KEY, "session");
}

#[test]
fn a_stored_account_reports_when_its_session_must_be_renewed() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);
    let stored: StoredAccount = store.remember(&account("uuid-a", "Alex"), NOW).unwrap();

    assert!(!stored.needs_refresh(NOW));
    assert!(stored.needs_refresh(NOW + 86400));
    assert!(stored.is_expired(NOW + 86400));
}

#[test]
fn a_sign_in_that_stopped_short_is_kept_so_it_can_be_retried() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(&dir);

    assert!(store.pending().unwrap().is_none());

    store
        .remember_pending(&Secret::new("microsoft-refresh-token"))
        .unwrap();

    assert_eq!(
        store
            .pending()
            .unwrap()
            .map(|held| held.expose().to_string()),
        Some("microsoft-refresh-token".to_string()),
        "a sign in that reached Microsoft but not Mojang must be resumable without a new code"
    );

    store.forget_pending().unwrap();
    assert!(store.pending().unwrap().is_none());
}

#[test]
fn a_file_backed_store_keeps_secrets_out_of_the_registry_and_off_other_users() {
    let dir = tempfile::tempdir().unwrap();
    let credentials = dir.path().join("credentials.json");
    let store = AccountStore::new(
        dir.path().join("accounts.json"),
        Box::new(FileSecrets::new(&credentials)),
    );

    store.remember(&account("uuid-a", "Player"), NOW).unwrap();

    let registry = std::fs::read_to_string(dir.path().join("accounts.json")).unwrap();
    assert!(
        !registry.contains("Player-refresh-token") && !registry.contains("Player-session-token"),
        "the registry must never carry credentials, whichever store holds them"
    );

    assert_eq!(
        store
            .refresh_token("uuid-a")
            .unwrap()
            .map(|held| held.expose().to_string()),
        Some("Player-refresh-token".to_string())
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&credentials)
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "a credentials file has to be unreadable by anyone else on the machine"
        );
    }
}

#[test]
fn a_store_that_cannot_be_written_is_not_treated_as_usable() {
    let dir = tempfile::tempdir().unwrap();
    let unwritable = dir.path().join("nope").join("deeper");
    std::fs::create_dir_all(&unwritable).unwrap();

    assert!(acelus_auth::store::usable(&FileSecrets::new(
        unwritable.join("credentials.json")
    )));
    assert!(acelus_auth::store::usable(&MemorySecrets::new()));
}
