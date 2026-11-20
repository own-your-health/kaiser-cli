//! `kp registration`: what to enter in Kaiser's developer portal to register
//! an app for kp, and how to store what it issues. `docs/registration.md`
//! walks through the portal; this command prints the values, with the
//! redirect URI on the configured port. It reads the config and no secret.

use pk_cli_auth::vault::Vault;
use pk_cli_core::{output, CliError};
use serde_json::{json, Value};

use super::{Ctx, BIN, REPO};
use crate::config::Config;
use crate::credentials;
use crate::kaiser::Environment;

pub const PORTAL: &str = "https://developer.kp.org";

/// The API to request in the portal's catalog, as the catalog names it.
pub const API_NAME: &str = "Patient Access API - Explanation of Benefits and Associated Resources";

pub const API_VERSION: &str = "Version 4.0.1 FHIR";

/// The step-by-step guide, where a stranger reading an error can open it.
pub fn guide() -> String {
    format!("https://github.com/{REPO}/blob/main/docs/registration.md")
}

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

/// The remedy for an environment with no client stored: the guide, the
/// command that prints the values, then the command that stores them.
pub fn missing_client_remedy(environment: Environment) -> String {
    let after = match environment {
        Environment::Production => ", once Kaiser approves the request",
        Environment::Sandbox => "",
    };
    format!(
        "register an app at {PORTAL} (`{BIN} registration` prints what to enter; {}), then run `{}`{after}",
        guide(),
        store_command(environment)
    )
}

/// Each environment's `{environment, stored}`, from whether its vault entry
/// exists: no Keychain read, so `info` and `registration` raise no prompt.
pub fn client_rows(vault: &Vault) -> Vec<Value> {
    Environment::ALL
        .into_iter()
        .map(|e| json!({ "environment": e.as_str(), "stored": credentials::client_on_disk(vault, e) }))
        .collect()
}

/// `registration/v1`'s payload.
pub fn values(config: &Config, vault: &Vault) -> Value {
    let redirect = config.redirect_uri();
    let stored = |e: Environment| credentials::client_on_disk(vault, e);
    json!({
        "portal": PORTAL,
        "guide": guide(),
        "fields": [
            {
                "step": "account",
                "field": "Company/Business Name",
                "value": null,
                "example": "Individual",
                "note": "required; someone registering for their own records can say so",
            },
            {
                "step": "persona",
                "field": "Persona",
                "value": "Developer",
                "note": "click the card itself, then Submit Persona Setup",
            },
            {
                "step": "application",
                "field": "Application Name (Public)",
                "value": null,
                "example": NAME_EXAMPLE,
                "note": "your choice; members see it on kp.org's list of apps with access, it cannot be edited after saving, and Kaiser asks for no names like test or validation",
            },
            {
                "step": "application",
                "field": "Application Website",
                "value": format!("https://github.com/{REPO}"),
            },
            {
                "step": "application",
                "field": "Application Description",
                "value": DESCRIPTION,
                "note": "cannot be edited after saving; members see it on kp.org",
            },
            {
                "step": "application",
                "field": "Persona or Type",
                "value": "Developer",
            },
            {
                "step": "api",
                "field": "API",
                "value": format!("{API_NAME} ({API_VERSION})"),
                "note": "Use this API, then Request Access for the application",
            },
            {
                "step": "sandbox",
                "field": "Sandbox Redirect URI",
                "value": redirect,
                "note": "Add sandbox uri, then Generate ID/Secret; the Client Secret dialog shows the secret once",
            },
            {
                "step": "production",
                "field": "Production Redirect URI",
                "value": redirect,
                "note": "read-only until Kaiser approves the request by email; then Generate ID/Secret, shown once",
            },
        ],
        "redirect_uri": redirect,
        "client": {
            "type": "confidential",
            "authentication": "client_secret_basic",
            "pkce": "S256",
            "launch": "standalone",
        },
        "environments": [
            {
                "environment": Environment::Sandbox.as_str(),
                "organization": Environment::Sandbox.org_id(),
                "scope": Environment::Sandbox.scope(),
                "approval": "none",
                "stored": stored(Environment::Sandbox),
                "store": store_command(Environment::Sandbox),
            },
            {
                "environment": Environment::Production.as_str(),
                "organization": Environment::Production.org_id(),
                "scope": Environment::Production.scope(),
                "approval": "Kaiser reviews the request and answers by email",
                "stored": stored(Environment::Production),
                "store": store_command(Environment::Production),
            },
        ],
    })
}

pub fn run(ctx: &Ctx) -> Result<(), CliError> {
    let config = ctx.config()?;
    let v = values(&config, &ctx.vault);
    output::emit(ctx.json(), "registration", v, render);
    Ok(())
}

fn render(v: &Value) {
    let s = |v: &Value| v.as_str().unwrap_or_default().to_string();
    println!("Register an app for kp at Kaiser's developer portal: {PORTAL}");
    println!("Step by step: {}", s(&v["guide"]));
    let mut step = String::new();
    for f in v["fields"].as_array().into_iter().flatten() {
        if s(&f["step"]) != step {
            step = s(&f["step"]);
            println!();
            println!(
                "{}",
                match step.as_str() {
                    "account" => "Create an Account (the sign-up's User Details)",
                    "persona" => "Persona Setup",
                    "application" => "Add Application",
                    "api" => "API Catalog",
                    "sandbox" => "Application Details: sandbox, right after the request",
                    _ => "Application Details: production, after Kaiser approves",
                }
            );
        }
        let value = match f["value"].as_str() {
            Some(value) => value.to_string(),
            None => format!("e.g. {}", s(&f["example"])),
        };
        println!("  {:<26} {value}", s(&f["field"]));
        if let Some(note) = f["note"].as_str() {
            println!("  {:<26} ({note})", "");
        }
    }
    println!();
    println!("Fixed by Kaiser, nothing to enter");
    println!(
        "  {:<26} confidential: client id and secret, sent as HTTP Basic",
        "Client"
    );
    println!("  {:<26} S256", "PKCE");
    for e in v["environments"].as_array().into_iter().flatten() {
        println!(
            "  {:<26} {}",
            format!("Scope ({})", s(&e["environment"])),
            s(&e["scope"])
        );
    }
    println!();
    println!("Store each client id and secret");
    for e in v["environments"].as_array().into_iter().flatten() {
        let state = match (e["stored"] == true, s(&e["environment"]).as_str()) {
            (true, _) => "stored",
            (false, "production") => "not stored; after Kaiser approves",
            (false, _) => "not stored",
        };
        println!(
            "  {:<26} {:<34} ({state})",
            s(&e["environment"]),
            s(&e["store"])
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_values_follow_the_configured_port_and_kaisers_scopes() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::new(pk_cli_auth::vault::MemoryKeys::new(), dir.path().join("v"));
        let mut config = Config::default();
        config.set("redirect_port", "8123").unwrap();
        let v = values(&config, &vault);
        assert_eq!(v["redirect_uri"], "http://127.0.0.1:8123/callback");
        let redirects: Vec<&Value> = v["fields"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["field"].as_str().unwrap().ends_with("Redirect URI"))
            .map(|f| &f["value"])
            .collect();
        assert_eq!(redirects, [&v["redirect_uri"], &v["redirect_uri"]]);
        let envs = v["environments"].as_array().unwrap();
        assert_eq!(envs[0]["scope"], Environment::Sandbox.scope());
        assert_eq!(envs[1]["scope"], Environment::Production.scope());
        assert_eq!(envs[0]["store"], "kp auth set-credential --sandbox");
        assert_eq!(envs[1]["store"], "kp auth set-credential");
        assert_eq!(envs[0]["stored"], false);
    }

    #[test]
    fn a_missing_client_names_the_guide_and_both_commands() {
        let r = missing_client_remedy(Environment::Sandbox);
        assert!(r.contains("docs/registration.md"), "{r}");
        assert!(r.contains("`kp registration`"), "{r}");
        assert!(r.ends_with("`kp auth set-credential --sandbox`"), "{r}");
        let r = missing_client_remedy(Environment::Production);
        assert!(
            r.contains("`kp auth set-credential`, once Kaiser approves"),
            "{r}"
        );
    }
}
