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
use crate::cli::{AuthCmd, LoginArgs, LogoutArgs, SetCredentialArgs, LOGIN_TIMEOUT};
use crate::credentials::{self, client_entry, legacy_client_account, ClientCredentials};
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
    let done = store_credential(ctx, args)?;
    output::emit(ctx.json(), "client-credential", done.clone(), |_| {
        println!(
            "Stored the {} client id and secret in kp's vault.",
            done["environment"].as_str().unwrap_or_default()
        )
    });
    Ok(())
}

fn store_credential(ctx: &Ctx, args: &SetCredentialArgs) -> Result<Value, CliError> {
    if args.source.stdin && args.source.from_env.is_some() {
        return Err(CliError::Usage(
            "pass exactly one of --stdin or --from-env".into(),
        ));
    }
    let environment = environment_of(args);
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
    store_pair(ctx, environment, args.overwrite, piped)
}

/// The registration `--sandbox` picks: the sandbox's, else production's.
fn environment_of(args: &SetCredentialArgs) -> Environment {
    if args.sandbox {
        Environment::Sandbox
    } else {
        Environment::Production
    }
}

/// Stores `piped`, or the pair the prompts ask for when there is none,
/// unless a client is stored for the environment and `overwrite` is not
/// given. `client-credential/v1`'s payload: `environment`, `stored`, and
/// the `vault_entry` it is stored as.
pub fn store_pair(
    ctx: &Ctx,
    environment: Environment,
    overwrite: bool,
    piped: Option<ClientCredentials>,
) -> Result<Value, CliError> {
    if !overwrite && credentials::read_client(&ctx.vault, environment)?.is_some() {
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
    Ok(json!({
        "environment": environment.as_str(),
        "stored": true,
        "vault_entry": client_entry(environment),
    }))
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

/// What a sign-in that ends without one says to do: the same command again,
/// for the same profile, organization and label.
fn login_again(
    profile: &pk_cli_config::profile::ProfileName,
    environment: Environment,
    args: &LoginArgs,
) -> String {
    let mut command = format!("{BIN} auth login --profile {profile}");
    if environment == Environment::Sandbox {
        command.push_str(" --org sandbox");
    }
    if let Some(label) = &args.label {
        command.push_str(&format!(" --label {label}"));
    }
    if args.no_browser {
        command.push_str(" --no-browser");
    }
    if args.timeout != LOGIN_TIMEOUT {
        command.push_str(&format!(" --timeout {}", args.timeout));
    }
    format!("run `{command}` again")
}

/// An error keeps the remedy it came with, and gets `again` otherwise.
fn or_again(e: CliError, again: &str) -> CliError {
    if e.remedy().is_some() {
        e
    } else {
        e.with_remedy(again)
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
    let again = login_again(&profile, environment, args);
    let code = pending
        .wait_for_code(Duration::from_secs(args.timeout * 60), |reason| {
            ctx.note(format!(
                "{reason}. Still waiting; press Ctrl-C to start again."
            ))
        })
        .map_err(|e| or_again(e, &again))?;
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
    ))
    .map_err(|e| or_again(e, &again))?;
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
    match ctx.state_file()?.load() {
        Ok(legacy) if !legacy.contexts.is_empty() || !legacy.clients.is_empty() => {
            v["unmigrated"] = json!({
                "sign_ins": legacy.contexts.len(),
                "clients": legacy.clients.len(),
                "remedy": format!("run `{BIN} profiles migrate <NAME>`"),
            });
        }
        Ok(_) => {}
        Err(e) => {
            v["unmigrated"] = json!({ "error": e.message(), "remedy": LEGACY_UNREADABLE });
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
        println!(
            "Profile:       {}",
            v["profile"].as_str().unwrap_or_default()
        );
        status.render();
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
        if let Some(e) = v["unmigrated"]["error"].as_str() {
            println!(
                "Unmigrated:    kp v0.1.x's state.json does not read: {e} ({LEGACY_UNREADABLE})"
            );
        } else if let Some(remedy) = v["unmigrated"]["remedy"].as_str() {
            println!("Unmigrated:    sign-ins and client credentials from kp v0.1.x ({remedy})");
        }
    }
    Ok(())
}

/// What to do about a kp v0.1.x `state.json` that does not read: it is the
/// only list of the Keychain items that release kept.
const LEGACY_UNREADABLE: &str = "it lists kp v0.1.x's Keychain items: remove those \
     (accounts `client/…` and `smart/…`) in Keychain Access, then delete the file";

fn logout(ctx: &Ctx, args: &LogoutArgs) -> Result<(), CliError> {
    let done = signed_out(ctx, args)?;
    if let Some(e) = done["legacy_error"]["message"].as_str() {
        ctx.note(format!(
            "warning: kp v0.1.x's state.json does not read ({e}), so kept: {LEGACY_UNREADABLE}"
        ));
    }
    output::emit(ctx.json(), "auth-logout", done.clone(), |v| {
        let members = v["signed_out"].as_array().map_or(0, Vec::len);
        let profile = v["profile"].as_str().unwrap_or_default();
        let kept: Vec<&str> = v["shared_kept_for"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        let forgot = if !args.forget {
            String::new()
        } else if kept.is_empty() {
            "; removed the client credentials".into()
        } else {
            format!(
                "; kept the client credentials, which profile(s) {} still use",
                kept.join(", ")
            )
        };
        println!("Signed out {members} member(s) of profile {profile}{forgot}.");
    });
    Ok(())
}

/// Signs the profile's chosen members out. With `--forget`, and only when
/// no other profile holds a sign-in, also removes what every profile
/// shares: the client pair, kp v0.1.x's Keychain items and the
/// `state.json` listing them, and the vault's key once the vault is empty.
///
/// `auth-logout/v1`'s payload: `profile`; `removed`, the vault entries
/// deleted; `signed_out`, the members' labels; `shared_kept_for`, the other
/// profiles whose sign-ins kept the shared items; `legacy_removed`, how many
/// v0.1.x items went; `state_cleared`; `vault_cleared`; and `legacy_error`
/// (`message`, `remedy`) when the v0.1.x `state.json` does not read, which
/// then stays.
fn signed_out(ctx: &Ctx, args: &LogoutArgs) -> Result<Value, CliError> {
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
    let mut done = json!({
        "profile": profile.as_str(),
        "removed": [],
        "signed_out": chosen.iter().map(Context::display).collect::<Vec<_>>(),
        "shared_kept_for": [],
        "legacy_removed": 0,
        "state_cleared": false,
        "vault_cleared": false,
    });
    if args.forget {
        let others = profiles_holding_sign_ins(ctx, &profile)?;
        if others.is_empty() {
            for environment in Environment::ALL {
                let entry = client_entry(environment);
                if ctx.vault.delete(&entry)? {
                    removed.push(entry);
                }
            }
            match forget_legacy(ctx, &profile)? {
                Ok((count, cleared)) => {
                    done["legacy_removed"] = count.into();
                    done["state_cleared"] = cleared.into();
                }
                Err(e) => {
                    done["legacy_error"] =
                        json!({ "message": e.message(), "remedy": LEGACY_UNREADABLE });
                }
            }
            // With nothing left in the vault, its key item goes too, so kp
            // leaves nothing in the Keychain.
            let left = ctx.vault.list()?;
            if left.names.is_empty() && left.unreadable.is_empty() {
                ctx.vault.clear()?;
                done["vault_cleared"] = true.into();
            }
        } else {
            done["shared_kept_for"] = others.into();
        }
    }
    done["removed"] = removed.into();
    Ok(done)
}

/// The other profiles that still hold a sign-in: a grant in the config, or
/// an entry in the vault (a leading `_` is no profile's).
fn profiles_holding_sign_ins(
    ctx: &Ctx,
    profile: &pk_cli_config::profile::ProfileName,
) -> Result<Vec<String>, CliError> {
    let config = ctx.config()?;
    let mut others = std::collections::BTreeSet::new();
    for p in config.profiles.list().profiles {
        let name = pk_cli_config::profile::ProfileName::parse(&p.name)?;
        if &name != profile && !ctx.settings(&config, &name)?.grants.is_empty() {
            others.insert(p.name);
        }
    }
    for entry in ctx.vault.names()? {
        if let Some((owner, _)) = entry.split_once('/') {
            if !owner.starts_with('_') && owner != profile.as_str() {
                others.insert(owner.to_string());
            }
        }
    }
    Ok(others.into_iter().collect())
}

/// Deletes the Keychain items a kp v0.1.x `state.json` lists, each client
/// pair and each sign-in, then the file: how many items went and whether
/// there was a file. The inner error is a file that does not read, which
/// stays, being the only list of its items; an item that cannot be deleted
/// is the outer error, and the file stays for the next run.
fn forget_legacy(
    ctx: &Ctx,
    profile: &pk_cli_config::profile::ProfileName,
) -> Result<Result<(usize, bool), CliError>, CliError> {
    let state_file = ctx.state_file()?;
    if !state_file.path().exists() {
        return Ok(Ok((0, false)));
    }
    let state = match state_file.load() {
        Ok(state) => state,
        Err(e) => return Ok(Err(e)),
    };
    let again = format!("run `{BIN} auth logout --profile {profile} --forget` again to finish");
    let mut count = 0;
    for environment in Environment::ALL {
        if ctx
            .legacy
            .delete(&legacy_client_account(environment))
            .map_err(|e| e.with_remedy(again.clone()))?
        {
            count += 1;
        }
    }
    for c in &state.contexts {
        let account = c.account();
        if ctx
            .lock
            .forget(|| ctx.legacy.delete(&account))
            .map_err(|e| e.with_remedy(again.clone()))?
        {
            count += 1;
        }
    }
    Ok(Ok((count, state_file.clear()?)))
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
        let pair = |id: &str, secret: &str| Some(ClientCredentials::new(id, secret).unwrap());
        store_pair(
            &ctx,
            Environment::Production,
            false,
            pair("client-one", "secret-one"),
        )
        .unwrap();
        let err = store_pair(
            &ctx,
            Environment::Production,
            false,
            pair("client-two", "secret-two"),
        )
        .unwrap_err();
        assert_eq!(err.exit_code(), 6);
        store_pair(
            &ctx,
            Environment::Production,
            true,
            pair("client-two", "secret-two"),
        )
        .unwrap();
        let stored = credentials::read_client(&ctx.vault, Environment::Production)
            .unwrap()
            .unwrap();
        assert_eq!(stored.client_id, "client-two");
    }

    fn grant(id: &str, label: &str) -> Context {
        Context {
            id: id.into(),
            organization: Environment::Sandbox.org_id().into(),
            label: Some(label.into()),
            relationship: None,
            fhir_user: None,
            signed_in_at: "2020-01-01T00:00:00Z".into(),
        }
    }

    fn forget() -> LogoutArgs {
        LogoutArgs {
            who: PatientArgs::default(),
            forget: true,
        }
    }

    fn keys(v: &Value) -> Vec<&str> {
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        keys
    }

    fn source(stdin: bool, from_env: Option<&str>, sandbox: bool) -> SetCredentialArgs {
        SetCredentialArgs {
            source: pk_cli_secrets::SecretSourceArgs {
                stdin,
                from_env: from_env.map(Into::into),
            },
            sandbox,
            overwrite: false,
        }
    }

    #[test]
    fn sandbox_picks_the_sandbox_registration() {
        assert_eq!(
            environment_of(&source(true, None, true)),
            Environment::Sandbox
        );
        assert_eq!(
            environment_of(&source(true, None, false)),
            Environment::Production
        );
    }

    #[test]
    fn stdin_and_from_env_together_are_refused_before_any_read() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        // The variable is never set: the refusal comes before it is read.
        let err =
            store_credential(&ctx, &source(true, Some("KP_TEST_NEVER_SET"), false)).unwrap_err();
        assert_eq!(err.exit_code(), 2);
        assert!(ctx.vault.names().unwrap().is_empty());
    }

    #[test]
    fn set_credential_names_its_environment_and_vault_entry() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let pair = ClientCredentials::new("client-one", "secret-one").unwrap();
        assert_eq!(
            store_pair(&ctx, Environment::Sandbox, false, Some(pair)).unwrap(),
            json!({"environment": "sandbox", "stored": true, "vault_entry": "_client/sandbox"})
        );
    }

    #[test]
    fn logout_forget_keeps_what_profiles_share_while_another_holds_a_sign_in() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        credentials::store_client(
            &ctx.vault,
            Environment::Sandbox,
            &ClientCredentials::new("client-one", "secret-one").unwrap(),
        )
        .unwrap();
        let adult2 = ProfileName::parse("adult2").unwrap();
        let mut config = ctx.config().unwrap();
        config.profiles.add(adult2.clone());
        ctx.save_config(&config).unwrap();
        let mut settings = crate::config::ProfileSettings::default();
        settings.upsert(grant("pat-0002", "adult2"));
        ctx.save_settings(&adult2, &settings).unwrap();
        let state_file = ctx.state_file().unwrap();
        state_file.save(&crate::state::State::default()).unwrap();

        let done = signed_out(&ctx, &forget()).unwrap();
        assert_eq!(
            keys(&done),
            [
                "legacy_removed",
                "profile",
                "removed",
                "shared_kept_for",
                "signed_out",
                "state_cleared",
                "vault_cleared"
            ]
        );
        assert_eq!(done["shared_kept_for"], json!(["adult2"]));
        assert_eq!(done["removed"], json!([]));
        assert_eq!(done["vault_cleared"], false);
        assert!(credentials::read_client(&ctx.vault, Environment::Sandbox)
            .unwrap()
            .is_some());
        assert!(state_file.path().exists());

        // adult2 signed out too: now the shared items go.
        let mut adult2_ctx = ctx;
        adult2_ctx.profile = Some("adult2".into());
        let done = signed_out(&adult2_ctx, &forget()).unwrap();
        assert_eq!(done["shared_kept_for"], json!([]));
        assert_eq!(done["removed"], json!(["_client/sandbox"]));
        assert_eq!(done["state_cleared"], true);
        assert_eq!(done["vault_cleared"], true);
    }

    #[test]
    fn logout_forget_deletes_the_v01_items_its_state_lists_then_the_file() {
        use pk_cli_auth::vault::KeyStore;
        let dir = tempfile::tempdir().unwrap();
        let mut ctx = ctx(dir.path());
        let legacy = MemoryKeys::new();
        let old = grant("pat-0001", "adult1");
        legacy.set(&old.account(), &Secret::new("{}")).unwrap();
        legacy
            .set(
                &legacy_client_account(Environment::Sandbox),
                &Secret::new("{}"),
            )
            .unwrap();
        ctx.legacy = Box::new(legacy);
        let state_file = ctx.state_file().unwrap();
        state_file
            .save(&crate::state::State {
                clients: vec![Environment::Sandbox],
                contexts: vec![old.clone()],
            })
            .unwrap();
        let done = signed_out(&ctx, &forget()).unwrap();
        assert_eq!(done["legacy_removed"], 2);
        assert_eq!(done["state_cleared"], true);
        assert!(ctx.legacy.get(&old.account()).unwrap().is_none());
        assert!(ctx
            .legacy
            .get(&legacy_client_account(Environment::Sandbox))
            .unwrap()
            .is_none());
        assert!(!state_file.path().exists());
    }

    #[test]
    fn logout_forget_keeps_a_v01_state_that_does_not_read_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let state_file = ctx.state_file().unwrap();
        std::fs::write(state_file.path(), "{broken").unwrap();
        let done = signed_out(&ctx, &forget()).unwrap();
        assert!(done["legacy_error"]["message"].is_string(), "{done}");
        assert_eq!(done["legacy_error"]["remedy"], LEGACY_UNREADABLE);
        assert_eq!(done["state_cleared"], false);
        assert!(state_file.path().exists());
    }

    #[test]
    fn status_counts_what_v01_left_and_reports_a_state_that_does_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let state_file = ctx.state_file().unwrap();
        let (_, v) = status_of(&ctx).unwrap();
        assert!(v.get("unmigrated").is_none());
        state_file
            .save(&crate::state::State {
                clients: vec![Environment::Sandbox],
                contexts: vec![grant("pat-0001", "adult1")],
            })
            .unwrap();
        let (_, v) = status_of(&ctx).unwrap();
        assert_eq!(
            v["unmigrated"],
            json!({"sign_ins": 1, "clients": 1, "remedy": "run `kp profiles migrate <NAME>`"})
        );
        std::fs::write(state_file.path(), "{broken").unwrap();
        let (_, v) = status_of(&ctx).unwrap();
        assert!(v["unmigrated"]["error"].is_string(), "{v}");
        assert_eq!(v["unmigrated"]["remedy"], LEGACY_UNREADABLE);
    }

    #[test]
    fn a_sign_in_that_ends_without_one_names_the_same_command() {
        let profile = ProfileName::parse("adult1").unwrap();
        let args = |org: &str, label: Option<&str>, no_browser, timeout| LoginArgs {
            org: org.into(),
            label: label.map(Into::into),
            no_browser,
            timeout,
        };
        assert_eq!(
            login_again(
                &profile,
                Environment::Production,
                &args("kaiser-health-plan", None, false, LOGIN_TIMEOUT)
            ),
            "run `kp auth login --profile adult1` again"
        );
        assert_eq!(
            login_again(
                &profile,
                Environment::Sandbox,
                &args("kaiser-health-plan-sandbox", Some("pvaluser1"), true, LOGIN_TIMEOUT)
            ),
            "run `kp auth login --profile adult1 --org sandbox --label pvaluser1 --no-browser` again"
        );
        assert_eq!(
            login_again(
                &profile,
                Environment::Production,
                &args("production", None, false, 30)
            ),
            "run `kp auth login --profile adult1 --timeout 30` again"
        );
        let kept = CliError::Auth("refused".into()).with_remedy("ask Kaiser");
        assert_eq!(or_again(kept, "again").remedy(), Some("ask Kaiser"));
        assert_eq!(
            or_again(CliError::Auth("timed out".into()), "again").remedy(),
            Some("again")
        );
    }

    #[test]
    fn config_is_written_owner_only() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = ctx(dir.path());
        let mut settings = crate::config::ProfileSettings::default();
        settings.upsert(grant("pat-0001", "adult1"));
        ctx.save_settings(&ProfileName::parse("adult1").unwrap(), &settings)
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(ctx.store.path().unwrap())
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
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
