//! Command implementations. `main` builds a [`Ctx`] and dispatches; the
//! tests build one over an in-memory vault, a scratch config directory and
//! lock, and a stand-in browser.

pub mod api;
pub mod auth;
pub mod config;
pub mod orgs;
pub mod patients;
pub mod records;

use std::path::PathBuf;
use std::sync::Arc;

use pk_cli_auth::refresh_lock::RefreshLock;
use pk_cli_config::ConfigStore;
use pk_cli_core::{CliError, CommonArgs};
use pk_cli_http::BlockingTransport;
use pk_cli_secrets::CredentialStore;

use crate::cli::PatientArgs;
use crate::config::Config;
use crate::credentials::{self, ClientCredentials, Vault};
use crate::kaiser::{self, Environment};
use crate::session::{Context, Live};
use crate::state::StateFile;

pub const BIN: &str = "kp";
pub const REPO: &str = "piekstra/kaiser-cli";

/// Names the Keychain service a run uses instead of `piekstra.kp`. For the
/// test suite and for dogfooding error paths: a service that holds nothing
/// reads as signed out without a Keychain prompt. It moves only the
/// Keychain; the config and state stay where they are.
pub const KEYCHAIN_SERVICE_ENV: &str = "KP_KEYCHAIN_SERVICE";

pub struct Ctx {
    pub common: CommonArgs,
    pub store: ConfigStore,
    pub vault: Arc<dyn Vault>,
    pub lock: RefreshLock,
    /// Opens the sign-in URL; returns whether something was launched.
    pub browser: Box<dyn Fn(&str) -> bool>,
    /// Listen on any loopback port, not the configured one (tests, whose
    /// mock server redirects wherever it is asked to).
    pub any_port: bool,
}

impl Ctx {
    /// The binary's context: the Keychain (or `KP_KEYCHAIN_SERVICE`), the
    /// family's refresh lock, `~/.config/kp`, the system browser.
    pub fn for_binary(common: CommonArgs, config: Option<PathBuf>) -> Result<Self, CliError> {
        Ok(Ctx {
            common,
            store: ConfigStore::new(BIN).with_override(config),
            vault: Arc::new(credential_store()),
            lock: RefreshLock::for_binary(BIN)?,
            browser: Box::new(|url| {
                pk_cli_auth::oauth::open_browser(url);
                true
            }),
            any_port: false,
        })
    }

    pub fn json(&self) -> bool {
        self.common.json
    }

    pub fn config(&self) -> Result<Config, CliError> {
        let config: Config = self.store.load()?;
        config.check()?;
        Ok(config)
    }

    pub fn state_file(&self) -> Result<StateFile, CliError> {
        Ok(StateFile::beside(&self.store.path()?))
    }

    pub fn contexts(&self) -> Result<Vec<Context>, CliError> {
        Ok(self.state_file()?.load()?.contexts)
    }

    /// The signed-in member `who` names (the only one, when one is held).
    pub fn signed_in(&self, who: &PatientArgs) -> Result<Context, CliError> {
        let org = who.org.as_deref().map(kaiser::resolve_org).transpose()?;
        let contexts = self.contexts()?;
        crate::session::pick(&contexts, who.patient.as_deref(), org).cloned()
    }

    /// The client credentials for an environment, or the auth error that
    /// says how to store them.
    pub fn client(&self, environment: Environment) -> Result<ClientCredentials, CliError> {
        credentials::read_client(self.vault.as_ref(), environment)?.ok_or_else(|| {
            CliError::Auth(format!(
                "no {} client id and secret are stored",
                environment.as_str()
            ))
            .with_remedy(match environment {
                Environment::Production => "run `kp auth set-credential`",
                Environment::Sandbox => "run `kp auth set-credential --sandbox`",
            })
        })
    }

    /// A context's sign-in, renewed when its token has lapsed.
    pub fn live(&self, context: &Context) -> Result<Live<BlockingTransport>, CliError> {
        let client = self.client(context.environment()?)?;
        Live::start(
            transport()?,
            self.vault.clone(),
            self.lock.clone(),
            client,
            context,
        )
        .map_err(|e| sign_in_again(e, context))
    }

    pub fn note(&self, msg: impl AsRef<str>) {
        if !self.common.quiet {
            eprintln!("{}", msg.as_ref());
        }
    }
}

/// A sign-in error's remedy as kp's own command for this member: a refused or
/// missing sign-in is `auth login`, a stored one that does not read is
/// `auth logout` then `auth login`, and one a newer kp wrote is
/// `self-update`. Other errors keep their remedy.
pub fn sign_in_again(e: CliError, context: &Context) -> CliError {
    let org = match context.environment() {
        Ok(Environment::Sandbox) => "sandbox",
        _ => kaiser::ORG_ID,
    };
    let label = context
        .label
        .as_deref()
        .map(|l| format!(" --label {l}"))
        .unwrap_or_default();
    let login = format!("`{BIN} auth login --org {org}{label}`");
    match e.remedy() {
        Some(pk_med_smart::remedy::UPDATE) => {
            return e.with_remedy(format!("run `{BIN} self-update`"));
        }
        Some(pk_med_smart::remedy::CLEAR_AND_SIGN_IN_AGAIN) => {
            // The member's id stays out of the message: a label, or the
            // placeholder `patients list` fills.
            let (who, found) = match context.label.as_deref() {
                Some(label) => (label.to_string(), String::new()),
                None => (
                    "<WHO>".to_string(),
                    format!(" (`{BIN} patients list` shows who)"),
                ),
            };
            return e.with_remedy(format!(
                "run `{BIN} auth logout --org {org} --patient {who}`{found}, then {login}"
            ));
        }
        _ => {}
    }
    if e.exit_code() != 3 {
        return e;
    }
    e.with_remedy(format!("run {login}"))
}

/// An error as one line for stderr, its remedy after it, for a command
/// whose stdout already carries its output.
pub fn report(e: &CliError) {
    eprintln!("{}", report_line(e));
}

/// cli-common's own spelling, `message — remedy`, as every other error
/// prints, so a caller (medrec) reads the remedy the same way from both.
fn report_line(e: &CliError) -> String {
    format!("error: {e}")
}

pub fn credential_store() -> CredentialStore {
    match std::env::var(KEYCHAIN_SERVICE_ENV) {
        Ok(service) if !service.trim().is_empty() => CredentialStore::new(service),
        _ => CredentialStore::for_binary(BIN),
    }
}

/// The transport for Kaiser's FHIR and OAuth endpoints: no redirects, the
/// family's 60 s timeout.
pub fn transport() -> Result<BlockingTransport, CliError> {
    BlockingTransport::new()
}

/// RFC 3339 for Unix seconds.
pub fn rfc3339(secs: u64) -> String {
    pk_cli_core::dates::fmt_rfc3339(secs as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reported_error_puts_its_remedy_after_an_em_dash() {
        let e = CliError::Auth("the sign-in was refused".into())
            .with_remedy("run `kp auth login --org sandbox --label adult1`");
        assert_eq!(
            report_line(&e),
            "error: the sign-in was refused \u{2014} run `kp auth login --org sandbox --label adult1`"
        );
        let plain = CliError::Upstream("Kaiser answered 503".into());
        assert_eq!(
            report_line(&plain),
            "error: network/upstream error: Kaiser answered 503"
        );
    }
}
