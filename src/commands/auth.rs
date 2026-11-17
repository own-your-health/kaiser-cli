//! `kp auth set-credential|login|status|logout`.

use std::io::IsTerminal;
use std::time::Duration;

use pk_cli_auth::oauth::OAuthClient;
use pk_cli_auth::{AuthLogin, AuthMethod, AuthStatus};
use pk_cli_core::{block_on, output, CliError};
use pk_cli_secrets::Secret;
use pk_med_fhir::FhirClient;
use pk_med_smart::{begin, discover, LaunchRequest, SessionStore, SmartApp, SmartSession};
use serde_json::{json, Value};

use super::{rfc3339, transport, Ctx, BIN};
use crate::cli::{AuthCmd, LoginArgs, LogoutArgs, SetCredentialArgs};
use crate::credentials::{self, client_entry, ClientCredentials};
use crate::kaiser::{self, Environment, KaiserQuirks};
use crate::session::{client_auth, now, Context};

pub fn run(ctx: &Ctx, cmd: &AuthCmd) -> Result<(), CliError> {
    match cmd {
        AuthCmd::SetCredential(args) => set_credential(ctx, args),
        AuthCmd::Login(args) => login(ctx, args),
        AuthCmd::Status => status(ctx),
        AuthCmd::Logout(args) => logout(ctx, args),
    }
}

fn set_credential(ctx: &Ctx, args: &SetCredentialArgs) -> Result<(), CliError> {
    if args.source.stdin && args.source.from_env.is_some() {
        return Err(CliError::Usage(
            "pass exactly one of --stdin or --from-env".into(),
        ));
    }
    let environment = if args.sandbox {
        Environment::Sandbox
    } else {
        Environment::Production
    };
    let interactive =
        !args.source.stdin && args.source.from_env.is_none() && std::io::stdin().is_terminal();
    // Input that comes from a flag or a pipe is checked before the Keychain
    // is opened; a prompt waits until the refusal below, so nobody types a
    // secret only to have it turned away.
    let piped = if interactive {
        None
    } else {
        Some(read_piped(args)?)
    };
    if !args.overwrite && credentials::read_client(&ctx.vault, environment)?.is_some() {
        return Err(CliError::ConfirmationRequired(format!(
            "a {} client is already stored",
            environment.as_str()
        ))
        .with_remedy("pass --overwrite to replace it"));
    }
    let client = match piped {
        Some(client) => client,
        None => prompt_client()?,
    };
    credentials::store_client(&ctx.vault, environment, &client)?;
    let done = json!({
        "environment": environment.as_str(),
        "stored": true,
        "vault_entry": client_entry(environment),
    });
    output::emit(ctx.json(), "client-credential", done, |_| {
        println!(
            "Stored the {} client id and secret in kp's vault.",
            environment.as_str()
        )
    });
    Ok(())
}

/// The id and secret at two prompts that do not echo.
fn prompt_client() -> Result<ClientCredentials, CliError> {
    let id = Secret::prompt("Kaiser client id (not echoed)")?;
    let secret = Secret::prompt("Kaiser client secret (not echoed)")?;
    ClientCredentials::new(id.expose(), secret.expose())
}

/// The pair from `--stdin`, `--from-env`, or stdin when it is not a terminal.
fn read_piped(args: &SetCredentialArgs) -> Result<ClientCredentials, CliError> {
    let pair = if args.source.stdin || args.source.from_env.is_some() {
        args.source.read(None)?
    } else {
        pk_cli_secrets::read_stdin()?
    };
    ClientCredentials::parse_pair(&pair)
}

/// A label names a person in this household (`adult1`), never read from a
/// record: short, printable, no spaces.
fn check_label(label: &str) -> Result<(), CliError> {
    let ok = !label.is_empty()
        && label.len() <= 40
        && label
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    if ok {
        Ok(())
    } else {
        Err(CliError::Usage(
            "a label is up to 40 letters, digits, `-`, `_` or `.` (`adult1`, `child1`)".into(),
        ))
    }
}

fn login(ctx: &Ctx, args: &LoginArgs) -> Result<(), CliError> {
    let environment = kaiser::resolve_org(&args.org)?;
    if let Some(label) = &args.label {
        check_label(label)?;
    }
    if args.timeout == 0 || args.timeout > 60 {
        return Err(CliError::Usage("--timeout is from 1 to 60 minutes".into()));
    }
    let config = ctx.config()?;
    let profile = ctx.profile(&config)?;
    let base = config.fhir_base();
    kaiser::require_server(environment, &base)?;
    let client = ctx.client(environment)?;
    let t = transport()?;
    let smart = block_on(discover(
        &t,
        &base,
        &KaiserQuirks,
        BIN,
        env!("CARGO_PKG_VERSION"),
    ))?;
    // The secret goes to the token endpoint the server names; it too must be
    // Kaiser's, or for the sandbox this machine's.
    kaiser::require_server(environment, &smart.authorization_endpoint)?;
    kaiser::require_server(environment, &smart.token_endpoint)?;
    ctx.lock.before_signin()?;
    let app = SmartApp::new(
        BIN,
        client_auth(&client),
        if ctx.any_port {
            Vec::new()
        } else {
            vec![config.redirect_port()]
        },
    );
    let pending = begin(
        &app,
        LaunchRequest {
            config: &smart,
            fhir_base: &base,
            aud: None,
            scopes: environment.scopes(),
            quirks: &KaiserQuirks,
        },
    )?;
    // Chrome in the profile's own browser profile, so one kp.org account's
    // cookies never sign in another. Kaiser's authorize step is its own
    // sign-in (pfam.kp.org), so no page opens before it.
    let window = if args.no_browser {
        None
    } else {
        match (ctx.window)(&profile, pending.url()) {
            Ok(window) => Some(window),
            Err(e) => {
                eprintln!("error: Chrome did not open: {e}");
                eprintln!(
                    "warning: any other browser shares its cookies across kp's profiles: sign \
                     out of kp.org there first, or Kaiser may sign in another profile's account"
                );
                None
            }
        }
    };
    if window.is_some() {
        ctx.note(format!(
            "Signing in to Kaiser in Chrome (profile {profile}); waiting for it to finish."
        ));
    } else {
        ctx.note("Open this address in a browser and sign in to kp.org:");
        eprintln!("{}", pending.url());
    }
    let code = pending.wait_for_code(Duration::from_secs(args.timeout * 60), |reason| {
        ctx.note(format!(
            "{reason}. Still waiting; press Ctrl-C to start again."
        ))
    })?;
    // Chrome quits, writing the cookies it received to the profile's
    // browser profile.
    drop(window);
    let oauth = OAuthClient::new(t.clone(), BIN, env!("CARGO_PKG_VERSION"));
    let session = block_on(pending.exchange(
        &oauth,
        &client_auth(&client),
        &code,
        environment.org_id(),
        now(),
    ))?;
    if session.patient.as_deref().is_none_or(str::is_empty) {
        return Err(CliError::Upstream(
            "Kaiser's token answer named no member (the launch/patient context); nothing was stored"
                .into(),
        ));
    }
    // The grant first: it is the index `logout` removes vault entries by, so
    // a session never exists without one. A grant whose session failed to
    // store reads as signed out, and is removed if it is new.
    let context = Context::from_session(&session, args.label.clone(), rfc3339(now()));
    let mut settings = ctx.settings(&ctx.config()?, &profile)?;
    let existed = settings
        .grants
        .iter()
        .any(|c| c.organization == context.organization && c.id == context.id);
    settings.upsert(context.clone());
    ctx.save_settings(&profile, &settings)?;
    let sessions = ctx.sessions(&profile);
    if let Err(e) = ctx.lock.store_signin(|| sessions.save(&session)) {
        if !existed {
            settings.remove(&context.organization, &context.id);
            let _ = ctx.save_settings(&profile, &settings);
        }
        return Err(e);
    }
    let verified = verify(&t, &session, &context.id);
    if !verified {
        ctx.note(
            "Signed in, but reading the member's Patient record failed; `kp auth status` shows the sign-in.",
        );
    }
    let mut v = AuthLogin::stored(verified).to_json();
    v["profile"] = profile.as_str().into();
    v["patient"] = serde_json::to_value(context.patient_context()).unwrap_or_default();
    output::emit(ctx.json(), "auth-login", v, |_| {
        println!(
            "Signed in member {} at {} in profile {profile}.",
            context.display(),
            context.organization
        )
    });
    Ok(())
}

/// One read of the member's own Patient record, kept nowhere: whether the
/// new sign-in works.
fn verify(t: &pk_cli_http::BlockingTransport, session: &SmartSession, patient: &str) -> bool {
    FhirClient::new(t, &session.fhir_base, BIN, env!("CARGO_PKG_VERSION"))
        .map(|c| c.bearer(session.bearer()))
        .and_then(|c| block_on(c.read("Patient", patient)))
        .is_ok()
}

/// `auth-status/v1` with `profile`, `clients` and `patients` rows. An entry
/// that cannot be read is its row's `error`, not the command's failure.
pub fn status_of(ctx: &Ctx) -> Result<(AuthStatus, Value), CliError> {
    let config = ctx.config()?;
    let profile = ctx.profile(&config)?;
    let settings = ctx.settings(&config, &profile)?;
    let sessions = ctx.sessions(&profile);
    let mut clients = Vec::new();
    let mut any_client = false;
    for environment in Environment::ALL {
        // The vault reads its key only when the entry exists, so a
        // logged-out status raises no prompt.
        let mut row = json!({ "environment": environment.as_str(), "stored": false });
        match credentials::read_client(&ctx.vault, environment) {
            Ok(found) => row["stored"] = found.is_some().into(),
            Err(e) => row["error"] = e.message().into(),
        }
        any_client |= row["stored"] == true;
        clients.push(row);
    }
    let mut rows = Vec::new();
    let mut any_stored = false;
    let mut any_usable = false;
    for c in &settings.grants {
        let mut row = serde_json::to_value(c.patient_context()).unwrap_or_default();
        let session = match sessions.load(&c.account()) {
            Ok(session) => session,
            Err(e) => {
                let e = super::sign_in_again(e, &profile, c);
                row["error"] = e.message().into();
                if let Some(remedy) = e.remedy() {
                    row["remedy"] = remedy.into();
                }
                None
            }
        };
        let usable = session.as_ref().is_some_and(|s| {
            (s.refused_at.is_none() && !s.tokens.is_expired(now(), 0)) || s.renewable()
        });
        any_stored |= session.is_some();
        any_usable |= usable;
        row["credential_in_keychain"] = session.is_some().into();
        row["usable"] = usable.into();
        row["renewable"] = session.as_ref().is_some_and(SmartSession::renewable).into();
        if let Some(at) = session.as_ref().and_then(|s| s.tokens.expires_at_rfc3339()) {
            row["token_expires_at"] = at.into();
        }
        rows.push(row);
    }
    let mut status = AuthStatus::new(true, any_usable, AuthMethod::BrowserSession);
    status.credential_in_keychain = Some(any_stored || any_client);
    status.session_valid = Some(any_usable);
    let mut v = status.to_json();
    v["profile"] = profile.as_str().into();
    v["clients"] = Value::Array(clients);
    v["patients"] = Value::Array(rows);
    // Sign-ins kp v0.1.x kept are not this profile's until moved.
    if let Ok(legacy) = ctx.state_file()?.load() {
        if !legacy.contexts.is_empty() || !legacy.clients.is_empty() {
            v["unmigrated"] = json!({
                "sign_ins": legacy.contexts.len(),
                "clients": legacy.clients.len(),
                "remedy": format!("run `{BIN} profiles migrate <NAME>`"),
            });
        }
    }
    Ok((status, v))
}

fn status(ctx: &Ctx) -> Result<(), CliError> {
    let (status, v) = status_of(ctx)?;
    let clients = v["clients"].as_array().cloned().unwrap_or_default();
    let rows = v["patients"].as_array().cloned().unwrap_or_default();
    if ctx.json() {
        output::json(&v);
    } else {
        status.render();
        println!(
            "Profile:       {}",
            v["profile"].as_str().unwrap_or_default()
        );
        for c in &clients {
            println!(
                "Client ({}): {}",
                c["environment"].as_str().unwrap_or_default(),
                if c["stored"] == true {
                    "stored".to_string()
                } else if let Some(e) = c["error"].as_str() {
                    format!("unreadable ({e})")
                } else {
                    "not stored (`kp auth set-credential`)".to_string()
                }
            );
        }
        if rows.is_empty() {
            println!(
                "Members:       none signed in (`kp auth login --profile {} --label <WHO>`)",
                v["profile"].as_str().unwrap_or_default()
            );
        } else {
            println!();
            let view: Vec<Value> = rows
                .iter()
                .map(|r| {
                    json!({
                        "member": r["label"].as_str().unwrap_or(r["id"].as_str().unwrap_or_default()),
                        "organization": r["organization"],
                        "relationship": r["relationship"],
                        "usable": r["usable"],
                        "renewable": r["renewable"],
                        "token_expires": r["token_expires_at"],
                        "error": r["error"],
                        "remedy": r["remedy"],
                    })
                })
                .collect();
            output::table(&view);
        }
        if let Some(remedy) = v["unmigrated"]["remedy"].as_str() {
            println!("Unmigrated:    sign-ins and client credentials from kp v0.1.x ({remedy})");
        }
    }
    Ok(())
}

fn logout(ctx: &Ctx, args: &LogoutArgs) -> Result<(), CliError> {
    let org = args
        .who
        .org
        .as_deref()
        .map(kaiser::resolve_org)
        .transpose()?;
    let config = ctx.config()?;
    let profile = ctx.profile(&config)?;
    let mut settings = ctx.settings(&config, &profile)?;
    let chosen: Vec<Context> = match &args.who.patient {
        Some(q) => vec![crate::session::pick(&settings.grants, Some(q), org, &profile)?.clone()],
        None => settings
            .grants
            .iter()
            .filter(|c| org.is_none_or(|e| c.organization == e.org_id()))
            .cloned()
            .collect(),
    };
    let sessions = ctx.sessions(&profile);
    let mut removed = Vec::new();
    for c in &chosen {
        let account = c.account();
        if ctx.lock.forget(|| sessions.delete(&account))? {
            removed.push(sessions.entry_name(&account)?);
        }
        settings.remove(&c.organization, &c.id);
    }
    ctx.save_settings(&profile, &settings)?;
    let mut state_cleared = false;
    let mut vault_cleared = false;
    if args.forget {
        for environment in Environment::ALL {
            let entry = client_entry(environment);
            if ctx.vault.delete(&entry)? {
                removed.push(entry);
            }
        }
        state_cleared = ctx.state_file()?.clear()?;
        // With nothing left in the vault, its key item goes too, so kp
        // leaves nothing in the Keychain.
        let left = ctx.vault.list()?;
        if left.names.is_empty() && left.unreadable.is_empty() {
            ctx.vault.clear()?;
            vault_cleared = true;
        }
    }
    let done = json!({
        "profile": profile.as_str(),
        "removed": removed,
        "signed_out": chosen.iter().map(Context::display).collect::<Vec<_>>(),
        "state_cleared": state_cleared,
        "vault_cleared": vault_cleared,
    });
    output::emit(ctx.json(), "auth-logout", done, |v| {
        let members = v["signed_out"].as_array().map_or(0, Vec::len);
        println!(
            "Signed out {members} member(s) of profile {profile}{}.",
            if args.forget {
                "; removed the client credentials"
            } else {
                ""
            }
        );
    });
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::cli::PatientArgs;
    use crate::config::Config;
    use pk_cli_auth::refresh_lock::RefreshLock;
    use pk_cli_auth::vault::{MemoryKeys, Vault};
    use pk_cli_config::profile::ProfileName;
    use pk_cli_config::ConfigStore;
    use std::sync::Arc;

    /// A context over a scratch directory and an in-memory key, with the
    /// profile `adult1` configured and selected.
    pub(crate) fn ctx(dir: &std::path::Path) -> Ctx {
        let store = ConfigStore::new("kp").with_override(Some(dir.join("config.json")));
        let mut config = Config::default();
        config.profiles.add(ProfileName::parse("adult1").unwrap());
        store.save(&config).unwrap();
        Ctx {
            common: Default::default(),
            store,
            profile: Some("adult1".into()),
            vault: Arc::new(Vault::new(MemoryKeys::new(), dir.join("vault"))),
            legacy: Box::new(MemoryKeys::new()),
            lock: RefreshLock::at(dir.join("refresh.lock")),
            window: Box::new(|_, _| Err(CliError::Other("no browser in unit tests".into()))),
            forget_browser: Box::new(|_| Ok(false)),
            any_port: true,
        }
    }

    #[test]
    fn set_credential_refuses_to_replace_without_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        credentials::store_client(
            &ctx.vault,
            Environment::Production,
            &ClientCredentials::new("client-one", "secret-one").unwrap(),
        )
        .unwrap();
        // A variable only this test reads, so parallel tests do not race.
        std::env::set_var("KP_TEST_REFUSE_PAIR", "client-two:secret-two");
        let mut args = SetCredentialArgs {
            source: pk_cli_secrets::SecretSourceArgs {
                stdin: false,
                from_env: Some("KP_TEST_REFUSE_PAIR".into()),
            },
            sandbox: false,
            overwrite: false,
        };
        let err = set_credential(&ctx, &args).unwrap_err();
        assert_eq!(err.exit_code(), 6);
        args.overwrite = true;
        set_credential(&ctx, &args).unwrap();
        let stored = credentials::read_client(&ctx.vault, Environment::Production)
            .unwrap()
            .unwrap();
        assert_eq!(stored.client_id, "client-two");
    }

    #[test]
    fn logout_forget_removes_both_clients_the_old_state_and_the_key() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        for environment in Environment::ALL {
            credentials::store_client(
                &ctx.vault,
                environment,
                &ClientCredentials::new("client-one", "secret-one").unwrap(),
            )
            .unwrap();
        }
        let state_file = ctx.state_file().unwrap();
        state_file.save(&crate::state::State::default()).unwrap();
        logout(
            &ctx,
            &LogoutArgs {
                who: PatientArgs::default(),
                forget: true,
            },
        )
        .unwrap();
        assert!(ctx.vault.names().unwrap().is_empty());
        assert!(!state_file.path().exists());
        // The profile stays configured: `profiles remove` removes it.
        assert!(ctx
            .config()
            .unwrap()
            .profiles
            .contains(&ProfileName::parse("adult1").unwrap()));
    }

    #[test]
    fn a_login_command_without_a_profile_is_a_usage_error_before_anything_else() {
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ctx(dir.path());
        ctx.profile = None;
        // KP_PROFILE is read too; no test sets it.
        let err = status_of(&ctx).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(err.to_string().contains("configured: adult1"), "{err}");
        ctx.profile = Some("adult2".into());
        let err = status_of(&ctx).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert_eq!(err.remedy(), Some("add it with `kp profiles add adult2`"));
    }

    #[test]
    fn labels_are_short_and_plain() {
        for ok in ["adult1", "child-2", "a.b_c"] {
            check_label(ok).unwrap();
        }
        let long = "x".repeat(41);
        for bad in ["", "has space", "ünïcode", long.as_str()] {
            assert_eq!(check_label(bad).unwrap_err().exit_code(), 2, "{bad}");
        }
    }
}
