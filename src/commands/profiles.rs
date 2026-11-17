//! `kp profiles list|add|remove|migrate`: the logins kp keeps, one per
//! kp.org account. Names are the household's own (`adult1`) and live in the
//! config only.

use pk_cli_auth::vault::Migration;
use pk_cli_config::profile::{self, ProfileName};
use pk_cli_core::{confirm, output, CliError};
use pk_med_smart::migrate_sessions;
use serde_json::{json, Value};

use super::{Ctx, BIN, PROFILE_ENV};
use crate::cli::ProfilesCmd;
use crate::config::ProfileSettings;
use crate::credentials::{client_entry, legacy_client_account};
use crate::kaiser::Environment;
use crate::session::Context;

pub fn run(ctx: &Ctx, cmd: &ProfilesCmd) -> Result<(), CliError> {
    match cmd {
        ProfilesCmd::List => list(ctx),
        ProfilesCmd::Add { name } => add(ctx, name),
        ProfilesCmd::Remove { name, force } => remove(ctx, name, *force),
        ProfilesCmd::Migrate { name } => migrate(ctx, name),
    }
}

fn list(ctx: &Ctx) -> Result<(), CliError> {
    let list = ctx.config()?.profiles.list();
    if ctx.json() {
        output::json(&serde_json::to_value(&list).expect("profile-list/v1 serializes"));
    } else if list.profiles.is_empty() {
        println!("No profile is configured (`{BIN} profiles add <NAME>`).");
    } else {
        for p in &list.profiles {
            println!("{}", p.name);
        }
    }
    Ok(())
}

fn add(ctx: &Ctx, name: &str) -> Result<(), CliError> {
    let done = added(ctx, name)?;
    let name = done["profile"].as_str().unwrap_or_default().to_string();
    output::emit(ctx.json(), "profile-add", done.clone(), |_| {
        if done["added"] == true {
            println!("Added profile {name}. Sign its members in with `{BIN} auth login --profile {name} --label <WHO>`.");
        } else {
            println!("Profile {name} is already configured.");
        }
    });
    Ok(())
}

/// `profile-add/v1`'s payload: `profile`, and `added`, false when it was
/// already configured.
fn added(ctx: &Ctx, name: &str) -> Result<Value, CliError> {
    let name = ProfileName::parse(name)?;
    let mut config = ctx.config()?;
    let added = config.profiles.add(name.clone());
    if added {
        ctx.save_config(&config)?;
    }
    Ok(json!({ "profile": name.as_str(), "added": added }))
}

fn remove(ctx: &Ctx, name: &str, force: bool) -> Result<(), CliError> {
    let done = removed(ctx, name, force)?;
    output::emit(ctx.json(), "profile-remove", done.clone(), |_| {
        println!(
            "Removed profile {} and its {} sign-in(s).",
            done["profile"].as_str().unwrap_or_default(),
            done["sign_ins_removed"]
        )
    });
    Ok(())
}

/// Its sign-ins, its kept browser profile, then its config, after asking.
/// The config goes last, so a step that fails leaves the profile listed
/// and running the command again finishes it. `profile-remove/v1`'s
/// payload: `profile`, `sign_ins_removed`, `browser_profile_removed`.
fn removed(ctx: &Ctx, name: &str, force: bool) -> Result<Value, CliError> {
    let config = ctx.config()?;
    // An unknown name is the same usage error `--profile` gives.
    let name = profile::resolve_from(Some(name), None, PROFILE_ENV, &config.profiles, BIN)?;
    confirm::gate(
        &ctx.common,
        force,
        &format!("Remove profile {name}, its sign-ins and its browser profile?"),
    )?;
    let again = format!("run `{BIN} profiles remove {name} --force` again to finish");
    let sessions = ctx.sessions(&name);
    let sign_ins = ctx
        .lock
        .forget(|| sessions.delete_all())
        .map_err(|e| e.with_remedy(again.clone()))?;
    let browser_profile = (ctx.forget_browser)(&name).map_err(|e| e.with_remedy(again))?;
    let mut config = ctx.config()?;
    config.profiles.remove(&name);
    ctx.save_config(&config)?;
    Ok(json!({
        "profile": name.as_str(),
        "sign_ins_removed": sign_ins,
        "browser_profile_removed": browser_profile,
    }))
}

fn migrate(ctx: &Ctx, name: &str) -> Result<(), CliError> {
    let done = migrated(ctx, name)?;
    output::emit(ctx.json(), "profile-migrate", done.clone(), |_| {
        println!(
            "Profile {}: moved {} sign-in(s) and {} client registration(s) into kp's vault.",
            done["profile"].as_str().unwrap_or_default(),
            done["sessions"]["moved"],
            done["clients"]["moved"]
        )
    });
    Ok(())
}

/// Moves what kp v0.1.x kept, one Keychain item each and listed in
/// `state.json`, into the vault: the client credentials as the shared
/// `_client/<environment>` entries, the sign-ins into the profile with
/// their rows as its grants. No network, no new approval. Each old item a
/// newly built kp reads costs one Keychain dialog, once.
///
/// The items move first and the config is written once, after: a move that
/// fails leaves no profile behind, and `state.json`, still in place, lets a
/// second run finish (a moved item is found in the vault and kept).
/// `profile-migrate/v1`'s payload: `profile`, `added`, `grants`,
/// `sessions` and `clients` (each `moved`, `kept`, `absent`),
/// `state_cleared`.
fn migrated(ctx: &Ctx, name: &str) -> Result<Value, CliError> {
    let name = ProfileName::parse(name)?;
    let state_file = ctx.state_file()?;
    let state = state_file.load()?;
    // The config must read before anything moves.
    ctx.config()?;
    let pairs: Vec<(String, String)> = Environment::ALL
        .iter()
        .map(|e| (legacy_client_account(*e), client_entry(*e)))
        .collect();
    let pairs: Vec<(&str, &str)> = pairs
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let clients = ctx.vault.migrate_from(ctx.legacy.as_ref(), &pairs)?;
    let accounts: Vec<String> = state.contexts.iter().map(Context::account).collect();
    let sessions = migrate_sessions(
        ctx.legacy.as_ref(),
        &ctx.sessions(&name),
        &accounts,
        &ctx.lock,
    )?;
    let mut config = ctx.config()?;
    let added = config.profiles.add(name.clone());
    let mut settings: ProfileSettings = config.profiles.settings(&name)?;
    for c in &state.contexts {
        settings.upsert(c.clone());
    }
    config.profiles.set_settings(&name, &settings)?;
    ctx.save_config(&config)?;
    let state_cleared = state_file.clear()?;
    Ok(json!({
        "profile": name.as_str(),
        "added": added,
        "grants": state.contexts.len(),
        "sessions": counts(&sessions),
        "clients": counts(&clients),
        "state_cleared": state_cleared,
    }))
}

fn counts(m: &Migration) -> Value {
    json!({ "moved": m.moved, "kept": m.kept, "absent": m.absent })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::auth::tests::ctx;
    use crate::credentials::ClientCredentials;
    use crate::state::State;
    use pk_cli_auth::vault::KeyStore;
    use pk_cli_secrets::Secret;
    use pk_med_smart::SessionStore;

    fn context(id: &str) -> Context {
        Context {
            id: id.into(),
            organization: Environment::Sandbox.org_id().into(),
            label: Some("adult1".into()),
            relationship: None,
            fhir_user: None,
            signed_in_at: "2020-01-01T00:00:00Z".into(),
        }
    }

    /// A session as kp v0.1.x stored it: one item at its account.
    fn legacy_session(id: &str) -> String {
        json!({
            "format": 1,
            "organization": Environment::Sandbox.org_id(),
            "patient": id,
            "fhir_base": "http://127.0.0.1:9/fhir",
            "issuer": null,
            "token_endpoint": "http://127.0.0.1:9/token",
            "client_id": "client-one",
            "scope": "patient/*.read",
            "tokens": { "access_token": "a", "token_type": "Bearer", "obtained_at": 1 },
        })
        .to_string()
    }

    #[test]
    fn migrate_moves_the_old_items_into_a_new_profile_and_removes_the_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ctx(dir.path());
        let legacy = pk_cli_auth::vault::MemoryKeys::new();
        let c = context("pat-0001");
        legacy
            .set(&c.account(), &Secret::new(legacy_session("pat-0001")))
            .unwrap();
        let client =
            serde_json::to_string(&ClientCredentials::new("client-one", "secret-one").unwrap())
                .unwrap();
        legacy
            .set(
                &legacy_client_account(Environment::Sandbox),
                &Secret::new(client),
            )
            .unwrap();
        ctx.legacy = Box::new(legacy);
        let state_file = ctx.state_file().unwrap();
        state_file
            .save(&State {
                clients: vec![Environment::Sandbox],
                contexts: vec![c.clone()],
            })
            .unwrap();

        let done = migrated(&ctx, "adult2").unwrap();
        assert_eq!(
            done,
            json!({
                "profile": "adult2",
                "added": true,
                "grants": 1,
                "sessions": {"moved": 1, "kept": 0, "absent": 0},
                "clients": {"moved": 1, "kept": 0, "absent": 1},
                "state_cleared": true,
            })
        );

        let adult2 = ProfileName::parse("adult2").unwrap();
        let config = ctx.config().unwrap();
        assert!(config.profiles.contains(&adult2));
        let settings: ProfileSettings = config.profiles.settings(&adult2).unwrap();
        assert_eq!(settings.grants, vec![c.clone()]);
        let session = ctx.sessions(&adult2).load(&c.account()).unwrap().unwrap();
        assert_eq!(session.patient.as_deref(), Some("pat-0001"));
        assert!(ctx.legacy.get(&c.account()).unwrap().is_none());
        assert!(
            crate::credentials::read_client(&ctx.vault, Environment::Sandbox)
                .unwrap()
                .is_some()
        );
        assert!(!state_file.path().exists());
        // Idempotent: a second run finds nothing to move and changes nothing.
        migrate(&ctx, "adult2").unwrap();
        assert!(ctx.sessions(&adult2).load(&c.account()).unwrap().is_some());
    }

    #[test]
    fn remove_takes_the_sign_ins_and_the_config_and_leaves_the_others() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        add(&ctx, "adult2").unwrap();
        let adult1 = ProfileName::parse("adult1").unwrap();
        let adult2 = ProfileName::parse("adult2").unwrap();
        let session = pk_med_smart::decode_session(&legacy_session("pat-0001")).unwrap();
        ctx.sessions(&adult1).save(&session).unwrap();
        ctx.sessions(&adult2).save(&session).unwrap();
        assert_eq!(
            removed(&ctx, "adult1", true).unwrap(),
            json!({"profile": "adult1", "sign_ins_removed": 1, "browser_profile_removed": false})
        );
        let config = ctx.config().unwrap();
        assert!(!config.profiles.contains(&adult1));
        assert!(config.profiles.contains(&adult2));
        assert!(ctx.sessions(&adult1).accounts().unwrap().is_empty());
        assert_eq!(ctx.sessions(&adult2).accounts().unwrap().len(), 1);
        assert_eq!(remove(&ctx, "adult1", true).unwrap_err().exit_code(), 2);
    }

    #[test]
    fn remove_without_force_off_a_terminal_asks_and_changes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ctx(dir.path());
        // `--json` never asks, whatever stdin is.
        ctx.common.json = true;
        assert_eq!(remove(&ctx, "adult1", false).unwrap_err().exit_code(), 6);
        assert!(ctx
            .config()
            .unwrap()
            .profiles
            .contains(&ProfileName::parse("adult1").unwrap()));
    }

    #[test]
    fn add_says_whether_it_added() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        assert_eq!(
            added(&ctx, "adult2").unwrap(),
            json!({"profile": "adult2", "added": true})
        );
        assert_eq!(added(&ctx, "adult2").unwrap()["added"], false);
    }

    /// A Keychain that refuses every read, as one whose dialog was denied.
    struct Refusing;

    impl KeyStore for Refusing {
        fn get(&self, _: &str) -> Result<Option<Secret>, CliError> {
            Err(CliError::Other("the Keychain refused".into()))
        }
        fn set(&self, _: &str, _: &Secret) -> Result<(), CliError> {
            Err(CliError::Other("the Keychain refused".into()))
        }
        fn delete(&self, _: &str) -> Result<bool, CliError> {
            Err(CliError::Other("the Keychain refused".into()))
        }
    }

    #[test]
    fn a_migrate_that_cannot_read_the_old_items_adds_no_profile_and_keeps_the_state() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ctx(dir.path());
        ctx.legacy = Box::new(Refusing);
        let state_file = ctx.state_file().unwrap();
        state_file
            .save(&State {
                clients: vec![Environment::Sandbox],
                contexts: vec![context("pat-0001")],
            })
            .unwrap();
        assert!(migrated(&ctx, "adult2").is_err());
        assert!(!ctx
            .config()
            .unwrap()
            .profiles
            .contains(&ProfileName::parse("adult2").unwrap()));
        assert!(state_file.path().exists());
    }

    #[test]
    fn a_remove_whose_browser_profile_stays_keeps_the_profile_and_says_to_run_it_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ctx(dir.path());
        ctx.forget_browser = Box::new(|_| Err(CliError::Other("Chrome holds the profile".into())));
        let err = removed(&ctx, "adult1", true).unwrap_err();
        assert_eq!(
            err.remedy(),
            Some("run `kp profiles remove adult1 --force` again to finish")
        );
        assert!(ctx
            .config()
            .unwrap()
            .profiles
            .contains(&ProfileName::parse("adult1").unwrap()));
    }

    #[test]
    fn a_profile_name_is_checked() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        for bad in ["", "Adult1", "_client", "has space", "a/b"] {
            assert_eq!(add(&ctx, bad).unwrap_err().exit_code(), 2, "{bad}");
        }
    }
}
