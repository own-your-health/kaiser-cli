//! The member sign-ins a profile holds: pk-med-smart's `SmartSession` in
//! kp's vault, one entry per (organization, member) named
//! `<profile>/<organization>/<member>` (`ProfileSessions`), and a grant for
//! each in the profile's settings (`Context`), so `patients list` reads no
//! keychain item.

use std::sync::{Arc, Mutex};

use pk_cli_auth::oauth::{ClientAuth, OAuthClient};
use pk_cli_auth::refresh_lock::RefreshLock;
use pk_cli_auth::vault::Vault;
use pk_cli_config::profile::ProfileName;
use pk_cli_core::transport::Transport;
use pk_cli_core::CliError;
use pk_cli_secrets::Secret;
use pk_med_records::dto::{kind, relationship};
use pk_med_records::PatientContext;
use pk_med_smart::{ProfileSessions, Renewer, SessionStore, SmartSession};
use serde::{Deserialize, Serialize};

use crate::credentials::ClientCredentials;
use crate::kaiser::{Environment, KaiserQuirks};

/// One grant in a profile's settings (and a row of a pre-profile
/// `state.json`): everything about a sign-in but its tokens.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Context {
    /// The member's FHIR `Patient.id` at Kaiser (`--patient`).
    pub id: String,
    /// kp's organization id (`kaiser-health-plan`, or the sandbox's).
    pub organization: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relationship: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fhir_user: Option<String>,
    /// RFC 3339.
    pub signed_in_at: String,
}

impl Context {
    pub fn from_session(
        session: &SmartSession,
        label: Option<String>,
        signed_in_at: String,
    ) -> Self {
        let id = session.patient.clone().unwrap_or_default();
        Context {
            relationship: relationship_of(session.fhir_user.as_deref(), &id),
            id,
            organization: session.organization.clone(),
            label,
            fhir_user: session.fhir_user.clone(),
            signed_in_at,
        }
    }

    pub fn account(&self) -> String {
        pk_med_smart::account(&self.organization, &self.id)
    }

    pub fn environment(&self) -> Result<Environment, CliError> {
        Environment::of_org(&self.organization).ok_or_else(|| {
            CliError::Other(format!(
                "state.json names an organization kp does not know ({})",
                self.organization
            ))
        })
    }

    /// The label, else the FHIR id.
    pub fn display(&self) -> String {
        self.label.clone().unwrap_or_else(|| self.id.clone())
    }

    /// The `patient-list/v1` item and the stream header's `patient`. The
    /// name is never read from the record.
    pub fn patient_context(&self) -> PatientContext {
        PatientContext {
            id: self.id.clone(),
            organization: self.organization.clone(),
            label: self.label.clone(),
            relationship: self.relationship.clone(),
            kind: Some(kind::MEMBER.to_string()),
            member_role: None,
            fhir_user: self.fhir_user.clone(),
            name: None,
            session: Some(self.account()),
        }
    }
}

/// `self` when the signed-in user is the member, `proxy` when it is someone
/// else (a parent acting for a child); unknown when Kaiser does not say.
fn relationship_of(fhir_user: Option<&str>, patient: &str) -> Option<String> {
    let user = fhir_user?;
    let own =
        user == format!("Patient/{patient}") || user.ends_with(&format!("/Patient/{patient}"));
    Some(
        if own {
            relationship::SELF
        } else {
            relationship::PROXY
        }
        .to_string(),
    )
}

/// Resolve `--patient` (and `--org`) against a profile's grants: with none
/// given and one held, that one.
pub fn pick<'a>(
    contexts: &'a [Context],
    patient: Option<&str>,
    org: Option<Environment>,
    profile: &ProfileName,
) -> Result<&'a Context, CliError> {
    let narrowed: Vec<Context> = contexts
        .iter()
        .filter(|c| org.is_none_or(|e| c.organization == e.org_id()))
        .cloned()
        .collect();
    if narrowed.is_empty() {
        // The remedy signs in where the caller asked. `--patient` is not
        // echoed: it may be a member's FHIR id rather than a label.
        let at = org.unwrap_or(Environment::Production).org_id();
        return Err(CliError::Auth(match org {
            Some(e) => format!(
                "profile {profile} has no member signed in at {}",
                e.org_id()
            ),
            None => format!("profile {profile} has no member signed in"),
        })
        .with_remedy(format!(
            "run `kp auth login --profile {profile} --org {at} --label <WHO>`"
        )));
    }
    let chosen = match patient {
        None if narrowed.len() == 1 => narrowed[0].clone(),
        None => {
            return Err(CliError::Usage(format!(
                "{} members are signed in; name one with --patient ({})",
                narrowed.len(),
                narrowed
                    .iter()
                    .map(Context::display)
                    .collect::<Vec<_>>()
                    .join(", ")
            )))
        }
        Some(q) => pk_cli_core::resolve::pick(
            &narrowed,
            q,
            |c| vec![c.id.clone()],
            |c| c.label.as_deref().unwrap_or(&c.id),
            "signed-in member",
        )?
        .clone(),
    };
    contexts
        .iter()
        .find(|c| c.organization == chosen.organization && c.id == chosen.id)
        .ok_or_else(|| CliError::Other("the chosen member vanished from the list".into()))
}

/// How kp authenticates to Kaiser's token endpoint: the confidential
/// client's id and secret as an HTTP Basic header.
pub fn client_auth(client: &ClientCredentials) -> ClientAuth {
    ClientAuth::SecretBasic {
        client_id: client.client_id.clone(),
        secret: client.secret(),
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// A sign-in in use: its session renewed when lapsed, and renewed again
/// when Kaiser refuses its token mid-run. Owns what renewal needs, so the
/// FHIR client can call it from its `bearer_with` source.
#[derive(Clone)]
pub struct Live<T> {
    pub account: String,
    vault: Arc<Vault>,
    profile: ProfileName,
    lock: RefreshLock,
    oauth: OAuthClient<T>,
    client: ClientCredentials,
    current: Arc<Mutex<SmartSession>>,
}

impl<T: Transport + Clone + Send + Sync + 'static> Live<T> {
    /// The context's session, renewed first when its token has lapsed.
    pub fn start(
        transport: T,
        vault: Arc<Vault>,
        profile: ProfileName,
        lock: RefreshLock,
        client: ClientCredentials,
        context: &Context,
    ) -> Result<Self, CliError> {
        let account = context.account();
        let sessions = ProfileSessions::new(&vault, profile.clone());
        // Where the stored sign-in points is checked before a renewal sends
        // the client id and secret to its token endpoint.
        if let Some(stored) = sessions.load(&account)? {
            let environment = context.environment()?;
            crate::kaiser::require_server(environment, &stored.fhir_base)?;
            crate::kaiser::require_server(environment, &stored.token_endpoint)?;
        }
        let oauth = OAuthClient::new(transport, crate::commands::BIN, env!("CARGO_PKG_VERSION"));
        let auth = client_auth(&client);
        let session =
            Renewer::new(&sessions, &lock, &oauth, &auth, &KaiserQuirks).fresh(&account, now())?;
        drop(sessions);
        Ok(Live {
            account,
            vault,
            profile,
            lock,
            oauth,
            client,
            current: Arc::new(Mutex::new(session)),
        })
    }

    pub fn session(&self) -> SmartSession {
        self.current.lock().expect("the session lock").clone()
    }

    pub fn fhir_base(&self) -> String {
        self.session().fhir_base
    }

    /// The bearer for the next request: the current token, or after Kaiser
    /// refused `refused`, a renewed one.
    pub fn bearer(&self, refused: Option<&Secret>) -> Result<Secret, CliError> {
        let Some(refused) = refused else {
            return Ok(self.session().bearer());
        };
        let auth = client_auth(&self.client);
        let sessions = ProfileSessions::new(&self.vault, self.profile.clone());
        let renewed = Renewer::new(&sessions, &self.lock, &self.oauth, &auth, &KaiserQuirks)
            .after_refusal(&self.account, refused.expose(), now())?;
        let bearer = renewed.bearer();
        *self.current.lock().expect("the session lock") = renewed;
        Ok(bearer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(id: &str, label: Option<&str>, org: Environment) -> Context {
        Context {
            id: id.into(),
            organization: org.org_id().into(),
            label: label.map(Into::into),
            relationship: None,
            fhir_user: None,
            signed_in_at: "2020-01-01T00:00:00Z".into(),
        }
    }

    fn adult1() -> ProfileName {
        ProfileName::parse("adult1").unwrap()
    }

    #[test]
    fn the_remedy_signs_in_where_the_caller_asked() {
        for who in ["adult1", "pat-0001"] {
            let err = pick(&[], Some(who), Some(Environment::Sandbox), &adult1()).unwrap_err();
            assert_eq!(err.exit_code(), 3);
            assert!(
                err.to_string().contains("kaiser-health-plan-sandbox"),
                "{err}"
            );
            assert!(!err.to_string().contains("pat-0001"), "{err}");
            assert_eq!(
                err.remedy(),
                Some("run `kp auth login --profile adult1 --org kaiser-health-plan-sandbox --label <WHO>`")
            );
        }
        let err = pick(&[], None, None, &adult1()).unwrap_err();
        assert_eq!(
            err.remedy(),
            Some("run `kp auth login --profile adult1 --org kaiser-health-plan --label <WHO>`")
        );
    }

    #[test]
    fn pick_takes_the_only_one_or_asks() {
        let p = adult1();
        let one = [ctx("pat-0001", Some("adult1"), Environment::Sandbox)];
        assert_eq!(pick(&one, None, None, &p).unwrap().id, "pat-0001");
        assert_eq!(pick(&[], None, None, &p).unwrap_err().exit_code(), 3);
        let two = [
            ctx("pat-0001", Some("adult1"), Environment::Sandbox),
            ctx("pat-0002", Some("child1"), Environment::Sandbox),
        ];
        assert_eq!(pick(&two, None, None, &p).unwrap_err().exit_code(), 2);
        assert_eq!(pick(&two, Some("child1"), None, &p).unwrap().id, "pat-0002");
        assert_eq!(
            pick(&two, Some("pat-0001"), None, &p).unwrap().id,
            "pat-0001"
        );
        assert_eq!(
            pick(&two, Some("nobody"), None, &p)
                .unwrap_err()
                .exit_code(),
            4
        );
        assert_eq!(
            pick(&two, None, Some(Environment::Production), &p)
                .unwrap_err()
                .exit_code(),
            3
        );
    }

    #[test]
    fn relationship_comes_from_fhir_user_only() {
        assert_eq!(relationship_of(None, "p1"), None);
        assert_eq!(
            relationship_of(Some("Patient/p1"), "p1").as_deref(),
            Some("self")
        );
        assert_eq!(
            relationship_of(Some("https://x.example/fhir/Patient/p1"), "p1").as_deref(),
            Some("self")
        );
        assert_eq!(
            relationship_of(Some("RelatedPerson/r1"), "p1").as_deref(),
            Some("proxy")
        );
    }

    #[test]
    fn a_context_is_a_member_with_its_keychain_account() {
        let c = ctx("pat-0001", Some("adult1"), Environment::Sandbox).patient_context();
        assert_eq!(c.kind.as_deref(), Some("member"));
        assert_eq!(
            c.session.as_deref(),
            Some("smart/kaiser-health-plan-sandbox/pat-0001")
        );
        assert!(c.name.is_none());
    }
}
