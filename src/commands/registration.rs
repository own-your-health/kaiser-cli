//! `kp registration`: what to enter in Kaiser's developer portal to register
//! an app for kp, as medical-common's `Registration` (`registration/v1`,
//! its DESIGN §3.8), so kp prints it as the other medical CLIs do.
//! `docs/registration.md` walks through the portal; this command prints the
//! values, with the redirect URI on the configured port. It reads the config
//! and no secret, and makes no request.

use pk_cli_auth::vault::Vault;
use pk_cli_core::{output, CliError};
use pk_med_smart::{missing_client, ClientStatus, ClientType, Registration, StoredIn};

use super::{Ctx, BIN, REPO};
use crate::config::Config;
use crate::credentials;
use crate::kaiser::Environment;

pub const VENDOR: &str = "Kaiser Permanente";

pub const PORTAL: &str = "https://developer.kp.org";

pub const DOC: &str = "docs/registration.md";

/// The API to request in the portal's catalog, as the catalog names it.
pub const API: &str =
    "Patient Access API - Explanation of Benefits and Associated Resources (Version 4.0.1 FHIR)";

pub const DESCRIPTION: &str = "A command-line tool that a Kaiser Permanente member runs on \
their own computer to download their own health-plan claims and coverage, and those of the \
family members they act for on kp.org, into their own files. It sends the records to no one \
else.";

pub const NAME_EXAMPLE: &str = "Family Health Records (kp)";

/// The command that stores an environment's client id and secret.
pub fn store_command(environment: Environment) -> String {
    match environment {
        Environment::Production => format!("{BIN} auth set-credential"),
        Environment::Sandbox => format!("{BIN} auth set-credential --sandbox"),
    }
}

/// What the form asks for, with `store` as the command that keeps the
/// result.
fn describe(config: &Config, store: String) -> Registration {
    let redirect = config.redirect_uri();
    Registration::new(
        BIN,
        VENDOR,
        PORTAL,
        DOC,
        ClientType::ConfidentialSymmetric,
        store,
    )
    .redirect_uri(redirect.clone())
    // The sandbox's scopes are production's and `sandbox`.
    .scopes(&Environment::Sandbox.scopes())
    .url("Application Website", format!("https://github.com/{REPO}"))
    .field_with_note(
        "Company/Business Name",
        "Individual",
        "on the sign-up's User Details page; a company is assumed, and someone registering for their own records can say so",
    )
    .field_with_note(
        "Persona Setup",
        "Developer",
        "click the card itself, then Submit Persona Setup",
    )
    .field_with_note(
        "Application Name (Public)",
        NAME_EXAMPLE,
        "or a name of your own; members see it on kp.org's list of apps with access, it cannot be edited after saving, and Kaiser asks for no names like test or validation",
    )
    .field_with_note(
        "Application Description",
        DESCRIPTION,
        "members see it on kp.org; it cannot be edited after saving",
    )
    .field("Persona or Type", "Developer")
    .field_with_note(
        "Use this API",
        API,
        "in the API Catalog, then Request Access for the application",
    )
    .field_with_note(
        "Sandbox Redirect URI",
        redirect.clone(),
        "Add sandbox uri, then Generate ID/Secret; the Client Secret dialog shows the secret once",
    )
    .field_with_note(
        "Production Redirect URI",
        redirect,
        "read-only until Kaiser approves the request; then Generate ID/Secret, shown once",
    )
    .note("The sign-up's password must be 8 to 16 characters; a longer one is refused.")
    .note("The sandbox needs no approval: its redirect URI and Generate ID/Secret work as soon as the API is requested.")
    .note("Production waits on Kaiser's review of the request, answered by email; Kaiser publishes no time frame.")
    .note(format!(
        "Store the sandbox's pair with `{}` and production's with `{}`; --stdin reads `<CLIENT_ID>:<CLIENT_SECRET>`.",
        store_command(Environment::Sandbox),
        store_command(Environment::Production)
    ))
}

/// The registration `kp registration` prints: both environments' store
/// commands in one.
pub fn registration(config: &Config) -> Registration {
    describe(config, format!("{BIN} auth set-credential [--sandbox]"))
}

/// The error for an environment with no client stored: exit 2, reason
/// `NO_CLIENT`, naming the guide, `kp registration` and that environment's
/// store command. The remedy names no redirect, so the default config does.
pub fn missing(environment: Environment) -> CliError {
    let registration = describe(&Config::default(), store_command(environment));
    missing_client(&registration, Some(environment.as_str()))
}

/// The `client` member of `info` and `auth status`: production's client
/// when it is stored, else the sandbox's, else none.
pub fn client_status(stored: impl Fn(Environment) -> bool) -> ClientStatus {
    [Environment::Production, Environment::Sandbox]
        .into_iter()
        .find(|e| stored(*e))
        .map_or_else(ClientStatus::none, |e| {
            ClientStatus::configured(ClientType::ConfidentialSymmetric, StoredIn::Vault)
                .registration(e.as_str())
        })
}

/// [`client_status`] from the vault's file names: no Keychain read, so a
/// newly installed kp's `info` raises no prompt.
pub fn client_on_disk(vault: &Vault) -> ClientStatus {
    client_status(|e| credentials::client_on_disk(vault, e))
}

pub fn run(ctx: &Ctx) -> Result<(), CliError> {
    let registration = registration(&ctx.config()?);
    if ctx.json() {
        output::json(
            &serde_json::to_value(&registration)
                .map_err(|e| CliError::Other(format!("serializing the registration: {e}")))?,
        );
    } else {
        print!("{}", registration.text());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_values_follow_the_configured_port_and_kaisers_scopes() {
        let mut config = Config::default();
        config.set("redirect_port", "8123").unwrap();
        let v = serde_json::to_value(registration(&config)).unwrap();
        assert_eq!(v["schema"], "registration/v1");
        assert_eq!(v["client_type"], "confidential-symmetric");
        assert_eq!(
            v["redirect_uris"],
            serde_json::json!(["http://127.0.0.1:8123/callback"])
        );
        for f in v["fields"].as_array().unwrap() {
            if f["label"].as_str().unwrap().ends_with("Redirect URI") {
                assert_eq!(f["value"], "http://127.0.0.1:8123/callback");
            }
        }
        let scopes: Vec<&str> = v["scopes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s.as_str().unwrap())
            .collect();
        for s in Environment::Sandbox.scope().split(' ') {
            assert!(scopes.contains(&s), "{s}");
        }
    }

    #[test]
    fn a_missing_client_names_its_environment_and_store_command() {
        let e = missing(Environment::Sandbox);
        assert_eq!(e.exit_code(), 2);
        assert_eq!(e.reason(), Some(pk_med_smart::registration::NO_CLIENT));
        assert!(e.message().contains("sandbox"), "{e}");
        let remedy = e.remedy().unwrap();
        assert!(remedy.contains("docs/registration.md"), "{remedy}");
        assert!(remedy.contains("`kp registration`"), "{remedy}");
        assert!(
            remedy.ends_with("`kp auth set-credential --sandbox`"),
            "{remedy}"
        );
        let e = missing(Environment::Production);
        assert!(e.remedy().unwrap().ends_with("`kp auth set-credential`"));
    }

    #[test]
    fn the_client_status_prefers_production() {
        assert_eq!(client_status(|_| false), ClientStatus::none());
        let only_sandbox = client_status(|e| e == Environment::Sandbox);
        assert_eq!(only_sandbox.registration.as_deref(), Some("sandbox"));
        assert_eq!(only_sandbox.stored_in, Some(StoredIn::Vault));
        let both = client_status(|_| true);
        assert_eq!(both.registration.as_deref(), Some("production"));
    }
}
