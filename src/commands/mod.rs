//! Command implementations. `main` builds a [`Ctx`] and dispatches; the
//! tests build one over a vault with an in-memory key, a scratch config
//! directory and lock, and a stand-in browser.

pub mod api;
pub mod auth;
pub mod config;
pub mod orgs;
pub mod patients;
pub mod profiles;
pub mod records;

use std::any::Any;
use std::path::PathBuf;
use std::sync::Arc;

use pk_cli_auth::refresh_lock::RefreshLock;
use pk_cli_auth::vault::{KeyStore, Vault};
use pk_cli_config::profile::{self, ProfileName};
use pk_cli_config::ConfigStore;
use pk_cli_core::{CliError, CommonArgs};
use pk_cli_http::BlockingTransport;
use pk_cli_secrets::CredentialStore;
use pk_med_smart::ProfileSessions;

use crate::cli::PatientArgs;
use crate::config::{Config, ProfileSettings};
use crate::credentials::{self, ClientCredentials};
use crate::kaiser::{self, Environment};
use crate::session::{Context, Live};
use crate::state::StateFile;

pub const BIN: &str = "kp";
pub const REPO: &str = "piekstra/kaiser-cli";

/// Names the keychain service a run uses instead of `piekstra.kp`. For the
/// test suite and for dogfooding: a service that holds nothing reads as
/// signed out without a keychain prompt. With it, the vault's entries live
/// beside the config file (`vault.<service>/`), so a test's entries never
/// meet the real vault's key.
pub const KEYCHAIN_SERVICE_ENV: &str = "KP_KEYCHAIN_SERVICE";

/// The environment's fallback for `--profile`.
pub const PROFILE_ENV: &str = "KP_PROFILE";

/// Opens the sign-in address in the profile's own browser; the window stays
/// open while the returned value lives.
pub type OpenWindow = Box<dyn Fn(&ProfileName, &str) -> Result<Box<dyn Any>, CliError>>;

/// Removes a profile's kept browser profile; whether there was one.
pub type ForgetBrowser = Box<dyn Fn(&ProfileName) -> Result<bool, CliError>>;

pub struct Ctx {
    pub common: CommonArgs,
    pub store: ConfigStore,
    /// `--profile`, before it is checked against the config.
    pub profile: Option<String>,
    pub vault: Arc<Vault>,
    /// Where releases before profiles kept each secret as its own keychain
    /// item, which `profiles migrate` moves from.
    pub legacy: Box<dyn KeyStore>,
    pub lock: RefreshLock,
    pub window: OpenWindow,
    pub forget_browser: ForgetBrowser,
    /// Listen on any loopback port, not the configured one (tests, whose
    /// mock server redirects wherever it is asked to).
    pub any_port: bool,
}

impl Ctx {
    /// The binary's context: kp's vault (or one under `KP_KEYCHAIN_SERVICE`),
    /// the family's refresh lock, `~/.config/kp`, Chrome in the profile's
    /// own browser profile.
    pub fn for_binary(
        common: CommonArgs,
        config: Option<PathBuf>,
        profile: Option<String>,
    ) -> Result<Self, CliError> {
        let store = ConfigStore::new(BIN).with_override(config);
        let (vault, legacy): (Vault, Box<dyn KeyStore>) = match keychain_override() {
            Some(service) => {
                let dir = store
                    .path()?
                    .parent()
                    .map(|d| d.join(format!("vault.{service}")))
                    .ok_or_else(|| CliError::Other("the config path has no directory".into()))?;
                (
                    Vault::new(CredentialStore::new(service.clone()), dir),
                    Box::new(CredentialStore::new(service)),
                )
            }
            None => (
                Vault::for_binary(BIN)?,
                Box::new(CredentialStore::for_binary(BIN)),
            ),
        };
        Ok(Ctx {
            common,
            store,
            profile,
            vault: Arc::new(vault),
            legacy,
            lock: RefreshLock::for_binary(BIN)?,
            window: Box::new(|profile, url| {
                pk_med_smart::SignInWindow::open_at(BIN, profile, url, &[])
                    .map(|w| Box::new(w) as Box<dyn Any>)
            }),
            forget_browser: Box::new(|profile| {
                pk_cli_browser::forget_named_profile(BIN, profile.as_str()).map_err(CliError::from)
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

    /// Write the config. Its profiles' grants carry each member's FHIR
    /// patient id, and `ConfigStore::save` (cli-common v0.33.1) writes it
    /// owner-only and whole, so a crash mid-write loses no profile.
    pub fn save_config(&self, config: &Config) -> Result<(), CliError> {
        self.store.save(config)
    }

    /// The profile this command runs as: `--profile`, else `KP_PROFILE`. It
    /// must be configured; there is no default.
    pub fn profile(&self, config: &Config) -> Result<ProfileName, CliError> {
        profile::resolve(self.profile.as_deref(), PROFILE_ENV, &config.profiles, BIN)
    }

    pub fn settings(
        &self,
        config: &Config,
        profile: &ProfileName,
    ) -> Result<ProfileSettings, CliError> {
        config.profiles.settings(profile)
    }

    /// Replace a profile's settings in the config file.
    pub fn save_settings(
        &self,
        profile: &ProfileName,
        settings: &ProfileSettings,
    ) -> Result<(), CliError> {
        let mut config = self.config()?;
        config.profiles.set_settings(profile, settings)?;
        self.save_config(&config)
    }

    pub fn sessions(&self, profile: &ProfileName) -> ProfileSessions<'_> {
        ProfileSessions::new(&self.vault, profile.clone())
    }

    /// `state.json` as releases before profiles kept it.
    pub fn state_file(&self) -> Result<StateFile, CliError> {
        Ok(StateFile::beside(&self.store.path()?))
    }

    /// The profile's member `who` names (the only one, when one is held).
    pub fn signed_in(
        &self,
        config: &Config,
        profile: &ProfileName,
        who: &PatientArgs,
    ) -> Result<Context, CliError> {
        let org = who.org.as_deref().map(kaiser::resolve_org).transpose()?;
        let settings = self.settings(config, profile)?;
        crate::session::pick(&settings.grants, who.patient.as_deref(), org, profile).cloned()
    }

    /// The client credentials for an environment, or the auth error that
    /// says how to store them.
    pub fn client(&self, environment: Environment) -> Result<ClientCredentials, CliError> {
        credentials::read_client(&self.vault, environment)?.ok_or_else(|| {
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

    /// A grant's sign-in, renewed when its token has lapsed.
    pub fn live(
        &self,
        profile: &ProfileName,
        context: &Context,
    ) -> Result<Live<BlockingTransport>, CliError> {
        let client = self.client(context.environment()?)?;
        Live::start(
            transport()?,
            self.vault.clone(),
            profile.clone(),
            self.lock.clone(),
            client,
            context,
        )
        .map_err(|e| sign_in_again(e, profile, context))
    }

    pub fn note(&self, msg: impl AsRef<str>) {
        if !self.common.quiet {
            eprintln!("{}", msg.as_ref());
        }
    }
}

fn keychain_override() -> Option<String> {
    std::env::var(KEYCHAIN_SERVICE_ENV)
        .ok()
        .filter(|s| !s.trim().is_empty())
}

/// A sign-in error's remedy as kp's own command for this member: a refused or
/// missing sign-in is `auth login`, a stored one that does not read is
/// `auth logout` then `auth login`, and one a newer kp wrote is
/// `self-update`. Other errors keep their remedy.
pub fn sign_in_again(e: CliError, profile: &ProfileName, context: &Context) -> CliError {
    let org = match context.environment() {
        Ok(Environment::Sandbox) => "sandbox",
        _ => kaiser::ORG_ID,
    };
    let label = context
        .label
        .as_deref()
        .map(|l| format!(" --label {l}"))
        .unwrap_or_default();
    let login = format!("`{BIN} auth login --profile {profile} --org {org}{label}`");
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
                    format!(" (`{BIN} patients list --profile {profile}` shows who)"),
                ),
            };
            return e.with_remedy(format!(
                "run `{BIN} auth logout --profile {profile} --org {org} --patient {who}`{found}, then {login}"
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
