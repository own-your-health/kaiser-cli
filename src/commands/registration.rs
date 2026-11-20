//! `kp registration [--sandbox]`: what to enter in Kaiser's developer portal
//! to register an app for kp, one medical-common `Registration` per
//! environment (`registration/v2`, its DESIGN §3.8), so kp prints it as the
//! other medical CLIs do. `docs/registration.md` walks through the portal;
//! this command prints the values, with the redirect URI on the configured
//! port. It reads the config and no secret, and makes no request.

use pk_cli_auth::vault::Vault;
use pk_cli_core::{output, CliError};
use pk_med_smart::{missing_client, ClientStatus, ClientType, Registration, StoredIn};

use super::{Ctx, BIN, REPO};
use crate::cli::RegistrationArgs;
use crate::config::Config;
use crate::credentials;
use crate::kaiser::Environment;

pub const VENDOR: &str = "Kaiser Permanente";

pub const PORTAL: &str = "https://developer.kp.org";

/// The API to request in the portal's catalog, as the catalog names it.
pub const API: &str =
    "Patient Access API - Explanation of Benefits and Associated Resources (Version 4.0.1 FHIR)";

pub const DESCRIPTION: &str = "A command-line tool that a Kaiser Permanente member runs on \
their own computer to download their own health-plan claims and coverage, and those of the \
family members they act for on kp.org, into their own files. It sends the records to no one \
else.";

pub const NAME_EXAMPLE: &str = "Family Health Records (kp)";

/// The step-by-step guide, where someone with a release binary and no
/// repository can open it.
pub fn doc() -> String {
    format!("https://github.com/{REPO}/blob/main/docs/registration.md")
}

/// The command that stores an environment's client id and secret.
pub fn store_command(environment: Environment) -> String {
    match environment {
        Environment::Production => format!("{BIN} auth set-credential"),
        Environment::Sandbox => format!("{BIN} auth set-credential --sandbox"),
    }
}

/// The command that prints an environment's registration.
pub fn command(environment: Environment) -> String {
    match environment {
        Environment::Production => format!("{BIN} registration"),
        Environment::Sandbox => format!("{BIN} registration --sandbox"),
    }
}

/// What the portal's forms ask for to get `environment`'s client: the
/// account, persona, application and API request are the same for both,
/// then that environment's redirect URI and client.
pub fn registration(config: &Config, environment: Environment) -> Registration {
    let redirect = config.redirect_uri();
    let common = Registration::new(
        BIN,
        VENDOR,
        PORTAL,
        doc(),
        ClientType::ConfidentialSymmetric,
        store_command(environment),
    )
    .registration(environment.as_str())
    .command(command(environment))
    .redirect_uri(redirect.clone())
    .scopes(&environment.scopes())
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
    .note("The sign-up's password must be 8 to 16 characters; a longer one is refused.");
    match environment {
        Environment::Sandbox => common
            .field_with_note(
                "Sandbox Redirect URI",
                redirect,
                "Add sandbox uri, then Generate ID/Secret; the Client Secret dialog shows the secret once",
            )
            .note("The sandbox needs no approval: its redirect URI and Generate ID/Secret work as soon as the API is requested.")
            .note("Its members are Kaiser's synthetic test members, listed with their passwords in the guide's Test Accounts table."),
        Environment::Production => common
            .field_with_note(
                "Production Redirect URI",
                redirect,
                "read-only until Kaiser approves the request; then Generate ID/Secret, shown once",
            )
            .note("Production waits on Kaiser's review of the request, answered by email; Kaiser publishes no time frame.")
            .note(format!(
                "The sandbox's client comes first and needs no approval: `{}`.",
                command(Environment::Sandbox)
            )),
    }
}

/// The error for an environment with no client stored: exit 2, reason
/// `no_client`, naming the guide, that environment's `registration`
/// command and its store command. The remedy names no redirect, so the
/// default config does.
pub fn missing(environment: Environment) -> CliError {
    missing_client(&registration(&Config::default(), environment))
}

/// The `clients` rows of `info` and `auth status`: production's, then the
/// sandbox's, each configured when `stored` says so.
pub fn clients(stored: impl Fn(Environment) -> bool) -> Vec<ClientStatus> {
    [Environment::Production, Environment::Sandbox]
        .into_iter()
        .map(|e| {
            let registration = registration(&Config::default(), e);
            ClientStatus::of(&registration, stored(e).then_some(StoredIn::Vault))
        })
        .collect()
}

/// [`clients`] from the vault's files (`Vault::contains`): no Keychain
/// read, so a newly installed kp's `info` raises no prompt. A vault
/// directory that cannot be read counts as nothing stored, so `info` still
/// answers; `auth status` reads the entries and reports the error.
pub fn clients_on_disk(vault: &Vault) -> Vec<ClientStatus> {
    clients(|e| credentials::client_on_disk(vault, e).unwrap_or(false))
}

pub fn run(ctx: &Ctx, args: &RegistrationArgs) -> Result<(), CliError> {
    let environment = if args.sandbox {
        Environment::Sandbox
    } else {
        Environment::Production
    };
    let registration = registration(&ctx.config()?, environment);
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
    fn each_environment_follows_the_configured_port_and_its_scopes() {
        let mut config = Config::default();
        config.set("redirect_port", "8123").unwrap();
        for (environment, label, command) in [
            (
                Environment::Production,
                "Production Redirect URI",
                "kp registration",
            ),
            (
                Environment::Sandbox,
                "Sandbox Redirect URI",
                "kp registration --sandbox",
            ),
        ] {
            let v = serde_json::to_value(registration(&config, environment)).unwrap();
            assert_eq!(v["schema"], "registration/v2");
            assert_eq!(v["registration"], environment.as_str());
            assert_eq!(v["command"], command);
            assert_eq!(v["store"], store_command(environment));
            assert_eq!(v["client_type"], "confidential-symmetric");
            assert!(v["doc"].as_str().unwrap().starts_with("https://"));
            assert_eq!(
                v["redirect_uris"],
                serde_json::json!(["http://127.0.0.1:8123/callback"])
            );
            let redirects: Vec<&serde_json::Value> = v["fields"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|f| f["label"].as_str().unwrap().ends_with("Redirect URI"))
                .collect();
            assert_eq!(redirects.len(), 1, "{environment:?}");
            assert_eq!(redirects[0]["label"], label);
            assert_eq!(redirects[0]["value"], "http://127.0.0.1:8123/callback");
            let scopes: Vec<&str> = v["scopes"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| s.as_str().unwrap())
                .collect();
            let scope = environment.scope();
            let want: Vec<&str> = scope.split(' ').collect();
            assert_eq!(scopes.len(), want.len());
            for s in want {
                assert!(scopes.contains(&s), "{s}");
            }
        }
    }

    #[test]
    fn a_missing_client_names_its_environment_and_commands() {
        let e = missing(Environment::Sandbox);
        assert_eq!(e.exit_code(), 2);
        assert_eq!(e.reason(), Some(pk_med_smart::registration::NO_CLIENT));
        assert!(e.message().contains("sandbox"), "{e}");
        let remedy = e.remedy().unwrap();
        assert!(remedy.contains(&doc()), "{remedy}");
        assert!(remedy.contains("`kp registration --sandbox`"), "{remedy}");
        assert!(
            remedy.contains("`kp auth set-credential --sandbox`"),
            "{remedy}"
        );
        let remedy = missing(Environment::Production)
            .remedy()
            .unwrap()
            .to_string();
        assert!(remedy.contains("`kp registration`"), "{remedy}");
        assert!(remedy.contains("`kp auth set-credential`"), "{remedy}");
    }

    #[test]
    fn clients_has_a_row_per_environment() {
        let none = clients(|_| false);
        assert_eq!(none.len(), 2);
        assert!(none.iter().all(|c| !c.configured));
        assert_eq!(none[0].registration.as_deref(), Some("production"));
        assert_eq!(none[1].registration.as_deref(), Some("sandbox"));
        let sandbox = clients(|e| e == Environment::Sandbox);
        assert!(!sandbox[0].configured);
        assert!(sandbox[1].configured);
        assert_eq!(sandbox[1].stored_in, Some(StoredIn::Vault));
    }
}
