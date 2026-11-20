//! Sign-in, renewal, export, `api` and logout in-process against
//! medical-common's mock SMART + FHIR server on 127.0.0.1, standing in for
//! Kaiser's. Sessions and client credentials live in a vault over a
//! temporary directory with an in-memory key; nothing touches the Keychain
//! or the network beyond 127.0.0.1. Every value is the mock's synthetic
//! member, signed in under the profile `adult1`.

use std::any::Any;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use pk_cli_auth::refresh_lock::RefreshLock;
use pk_cli_auth::vault::{MemoryKeys, Vault};
use pk_cli_config::profile::ProfileName;
use pk_cli_config::ConfigStore;
use pk_med_fhir::test_util::{http_get, Fault, MockServer, CLIENT_ID, CLIENT_SECRET, PATIENT_ID};
use pk_med_records::{Item, RecordsReader};
use pk_med_smart::SessionStore;

use kp::cli::{AuthCmd, ExportArgs, LoginArgs, LogoutArgs, PatientArgs, RecordsCmd};
use kp::commands::{self, Ctx};
use kp::config::{Config, ProfileSettings};
use kp::credentials::{self, ClientCredentials};
use kp::kaiser::Environment;

/// The browser's part: follow the authorization URL to its redirect and
/// deliver the redirect to kp's loopback listener, on another thread. It
/// opens for the profile the command runs as.
fn window() -> kp::commands::OpenWindow {
    Box::new(|profile: &ProfileName, url: &str| {
        assert_eq!(profile.as_str(), "adult1");
        let url = url.to_string();
        std::thread::spawn(move || {
            let answer = http_get(&url).expect("the mock answers the authorization");
            assert_eq!(answer.status, 302, "{}", answer.body);
            let back = answer.header("location").unwrap().to_string();
            http_get(&back).expect("kp's listener answers the redirect");
        });
        Ok(Box::new(()) as Box<dyn Any>)
    })
}

fn adult1() -> ProfileName {
    ProfileName::parse("adult1").unwrap()
}

/// The members `adult1` signed in.
fn settings(ctx: &Ctx) -> ProfileSettings {
    ctx.config().unwrap().profiles.settings(&adult1()).unwrap()
}

fn save(ctx: &Ctx, settings: &ProfileSettings) {
    ctx.save_settings(&adult1(), settings).unwrap();
}

/// The vault entry of a sign-in's account in `adult1`.
fn entry(account: &str) -> String {
    format!("adult1/{}", account.strip_prefix("smart/").unwrap())
}

fn ctx(dir: &Path, mock: &MockServer) -> Ctx {
    let store = ConfigStore::new("kp").with_override(Some(dir.join("config.json")));
    let mut config = Config::default();
    config.set("fhir_base", &mock.fhir_base()).unwrap();
    config.profiles.add(adult1());
    store.save(&config).unwrap();
    let ctx = Ctx {
        common: pk_cli_core::CommonArgs {
            quiet: true,
            ..Default::default()
        },
        store,
        profile: Some("adult1".into()),
        vault: Arc::new(Vault::new(MemoryKeys::new(), dir.join("vault"))),
        legacy: Box::new(MemoryKeys::new()),
        lock: RefreshLock::at(dir.join("refresh.lock")),
        window: window(),
        forget_browser: Box::new(|_| Ok(false)),
        any_port: true,
    };
    credentials::store_client(
        &ctx.vault,
        Environment::Sandbox,
        &ClientCredentials::new(CLIENT_ID, CLIENT_SECRET).unwrap(),
    )
    .unwrap();
    ctx
}

fn login(ctx: &Ctx, label: &str) {
    commands::auth::run(
        ctx,
        &AuthCmd::Login(LoginArgs {
            org: "sandbox".into(),
            label: Some(label.into()),
            no_browser: false,
            timeout: 1,
        }),
    )
    .unwrap();
}

fn export(ctx: &Ctx, dir: &Path, since: Option<&str>) -> (BTreeMap<String, u64>, Vec<String>) {
    let failed = commands::records::export(
        ctx,
        &ExportArgs {
            who: PatientArgs::default(),
            since: since.map(Into::into),
            ndjson: false,
            output: Some(dir.to_path_buf()),
        },
    )
    .unwrap();
    assert!(failed.is_none(), "{failed:?}");
    let file = std::fs::File::open(dir.join("records.ndjson")).unwrap();
    let mut reader = RecordsReader::new(std::io::BufReader::new(file));
    let header = reader.header().unwrap().clone();
    assert_eq!(header.organization.id, "kaiser-health-plan-sandbox");
    assert_eq!(header.patient.id, PATIENT_ID);
    assert_eq!(header.patient.label.as_deref(), Some("adult1"));
    assert_eq!(header.patient.kind.as_deref(), Some("member"));
    assert_eq!(header.source.cli, "kp");
    let mut skipped = Vec::new();
    for item in reader.by_ref() {
        if let Item::Trailer(t) = item.unwrap() {
            assert!(t.complete);
            skipped = t
                .skipped
                .iter()
                .map(|s| {
                    format!(
                        "{} {}",
                        s.code,
                        s.search
                            .clone()
                            .or(s.resource_type.clone())
                            .unwrap_or_default()
                    )
                })
                .collect();
        }
    }
    (reader.counts().clone(), skipped)
}

#[test]
fn sign_in_then_export_claims_and_what_they_name() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");

    let state = settings(&ctx);
    assert_eq!(state.grants.len(), 1);
    assert_eq!(state.grants[0].id, PATIENT_ID);
    assert_eq!(state.grants[0].organization, "kaiser-health-plan-sandbox");
    // The session is the profile's vault entry, and the key is the one item.
    assert_eq!(
        ctx.vault.names().unwrap(),
        [
            "_client/sandbox".to_string(),
            entry(&format!("smart/kaiser-health-plan-sandbox/{PATIENT_ID}"))
        ]
    );

    // Discovery read the CapabilityStatement first (Kaiser has no
    // .well-known), and the token request carried HTTP Basic.
    let seen = mock.requests();
    assert_eq!(seen[0].path(), "/fhir/metadata");
    let token = seen.iter().find(|r| r.path() == "/token").unwrap();
    assert!(token.header("authorization").unwrap().starts_with("Basic "));
    let authorize_scope = seen
        .iter()
        .find(|r| r.path() == "/authorize")
        .map(|r| r.target.clone())
        .unwrap();
    assert!(
        authorize_scope.contains("ExplanationOfBenefit"),
        "{authorize_scope}"
    );
    assert!(authorize_scope.contains("sandbox"), "{authorize_scope}");

    let out = dir.path().join("out");
    let (counts, skipped) = export(&ctx, &out, None);
    let want: BTreeMap<String, u64> = [
        ("Coverage", 1),
        ("Encounter", 1),
        ("ExplanationOfBenefit", 1),
        // The mock serves a fill from medical-common v0.6.5.
        ("MedicationDispense", 1),
        ("Organization", 2),
        ("Patient", 1),
        ("Practitioner", 1),
    ]
    .into_iter()
    .map(|(t, n)| (t.to_string(), n))
    .collect();
    assert_eq!(counts, want);
    assert!(skipped.is_empty(), "{skipped:?}");

    // Every search was by patient, and nothing went to another origin.
    for r in mock.requests() {
        if let Some(t) = [
            "ExplanationOfBenefit",
            "Coverage",
            "Encounter",
            "MedicationDispense",
        ]
        .iter()
        .find(|t| r.path() == format!("/fhir/{t}"))
        {
            assert!(
                r.target.contains(&format!("patient={PATIENT_ID}")),
                "{t}: {}",
                r.target
            );
        }
    }
}

#[test]
fn since_bounds_searches_by_last_updated() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");
    let out = dir.path().join("out");
    export(&ctx, &out, Some("2024-01-01"));
    let eob = mock
        .requests()
        .into_iter()
        .find(|r| r.path() == "/fhir/ExplanationOfBenefit")
        .unwrap();
    assert!(
        eob.target.contains("_lastUpdated=ge2024-01-01"),
        "{}",
        eob.target
    );
    let text = std::fs::read_to_string(out.join("records.ndjson")).unwrap();
    let header: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
    assert_eq!(header["since"], "2024-01-01");
    assert!(header["since_unbounded"]
        .as_array()
        .unwrap()
        .contains(&"Patient".into()));
}

#[test]
fn a_refused_token_is_renewed_once_and_the_export_goes_on() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");
    mock.fail(Fault::unauthorized("/fhir/ExplanationOfBenefit").times(1));
    let out = dir.path().join("out");
    let (counts, _) = export(&ctx, &out, None);
    assert_eq!(counts["ExplanationOfBenefit"], 1);
    let refreshes = mock
        .requests()
        .iter()
        .filter(|r| r.path() == "/token" && r.body.contains("grant_type=refresh_token"))
        .count();
    assert_eq!(refreshes, 1);
}

#[test]
fn a_refused_search_is_a_skip_not_a_stop() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");
    mock.fail(Fault::new(
        "GET",
        "/fhir/Encounter",
        403,
        r#"{"resourceType":"OperationOutcome","issue":[{"severity":"error","code":"forbidden","diagnostics":"scope not granted"}]}"#,
    ));
    mock.fail(Fault::unavailable("/fhir/MedicationDispense").times(5));
    let out = dir.path().join("out");
    let (counts, skipped) = export(&ctx, &out, None);
    assert!(!counts.contains_key("Encounter"));
    assert_eq!(counts["ExplanationOfBenefit"], 1);
    assert!(
        skipped.contains(&"forbidden Encounter".to_string()),
        "{skipped:?}"
    );
    assert!(
        skipped.contains(&"upstream MedicationDispense".to_string()),
        "{skipped:?}"
    );
}

#[test]
fn a_reference_longer_than_64_characters_is_read() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");
    let practitioner = include_str!("fixtures/long-id-practitioner.json");
    let id = serde_json::from_str::<serde_json::Value>(practitioner).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(id.len() > 64, "the fixture's id is {} characters", id.len());
    let searchset =
        include_str!("fixtures/long-id-eob-searchset.json").replace("PATIENT_ID", PATIENT_ID);
    mock.fail(Fault::new("GET", "/fhir/ExplanationOfBenefit", 200, searchset).times(5));
    let path = format!("/fhir/Practitioner/{id}");
    mock.fail(Fault::new("GET", &path, 200, practitioner).times(5));

    let out = dir.path().join("out");
    let (counts, skipped) = export(&ctx, &out, None);
    assert!(skipped.is_empty(), "{skipped:?}");
    assert_eq!(counts["ExplanationOfBenefit"], 1);
    // The long one, and the mock's own, which its other resources name.
    assert_eq!(counts["Practitioner"], 2);
    let seen: Vec<String> = mock.requests().iter().map(|r| r.target.clone()).collect();
    assert!(seen.iter().any(|t| t == &path), "{seen:?}");
}

#[test]
fn a_reference_fhir_does_not_allow_is_a_skip_that_never_names_it() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");
    let searchset =
        include_str!("fixtures/refused-id-eob-searchset.json").replace("PATIENT_ID", PATIENT_ID);
    mock.fail(Fault::new("GET", "/fhir/ExplanationOfBenefit", 200, searchset).times(5));

    let out = dir.path().join("out");
    let (counts, skipped) = export(&ctx, &out, None);
    assert_eq!(counts["ExplanationOfBenefit"], 1);
    assert_eq!(skipped, ["invalid_reference Practitioner"]);
    let written = std::fs::read_to_string(out.join("records.ndjson")).unwrap();
    let trailer = written.lines().last().unwrap();
    assert!(!trailer.contains("not a fhir id"), "{trailer}");
    assert!(trailer.contains("characters"), "{trailer}");
    assert!(!mock.requests().iter().any(|r| r.path().contains("not")));
}

#[test]
fn results_a_search_says_it_left_out_are_a_skip_and_the_rest_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");
    let line = "Client not authorized for ExplanationOfBenefit - Synthetic Sub-resource";
    mock.withhold("ExplanationOfBenefit", line);

    let out = dir.path().join("out");
    let (counts, skipped) = export(&ctx, &out, None);
    assert_eq!(counts["ExplanationOfBenefit"], 1);
    assert_eq!(counts["Practitioner"], 1);
    assert_eq!(skipped, ["forbidden ExplanationOfBenefit"]);
    let written = std::fs::read_to_string(out.join("records.ndjson")).unwrap();
    let trailer: serde_json::Value = serde_json::from_str(written.lines().last().unwrap()).unwrap();
    // Resources of the type the server did not serve: no search, no id.
    let skip = &trailer["skipped"][0];
    assert_eq!(skip["resource_type"], "ExplanationOfBenefit", "{skip}");
    assert_eq!(skip["reason"], line, "{skip}");
    assert!(skip.get("search").is_none(), "{skip}");
    assert!(skip.get("id").is_none(), "{skip}");
}

#[test]
fn a_missing_sign_in_names_kps_command_once() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");
    let state = settings(&ctx);
    assert!(ctx
        .sessions(&adult1())
        .delete(&state.grants[0].account())
        .unwrap());
    let err = commands::records::export(
        &ctx,
        &ExportArgs {
            who: PatientArgs::default(),
            since: None,
            ndjson: true,
            output: None,
        },
    )
    .unwrap_err();
    assert_eq!(err.exit_code(), 3, "{err}");
    assert!(!err.message().contains("auth login"), "{err}");
    assert_eq!(
        err.remedy(),
        Some("run `kp auth login --profile adult1 --org sandbox --label adult1`")
    );
}

#[test]
fn a_sign_in_a_newer_kp_stored_says_to_update() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");
    let state = settings(&ctx);
    let account = state.grants[0].account();
    let stored = ctx.vault.get(&entry(&account)).unwrap().unwrap();
    let mut session: serde_json::Value = serde_json::from_str(stored.expose()).unwrap();
    session["format"] = (pk_med_smart::SESSION_FORMAT + 1).into();
    ctx.vault
        .put(
            &entry(&account),
            &pk_cli_secrets::Secret::new(session.to_string().as_str()),
        )
        .unwrap();

    let err = commands::records::export(
        &ctx,
        &ExportArgs {
            who: PatientArgs::default(),
            since: None,
            ndjson: false,
            output: Some(dir.path().join("out")),
        },
    )
    .unwrap_err();
    assert_eq!(err.remedy(), Some("run `kp self-update`"), "{err}");
    assert!(!err.to_string().contains(PATIENT_ID), "{err}");
    let list = commands::patients::list(&ctx, true, None).unwrap();
    assert_eq!(list["items"][0]["remedy"], "run `kp self-update`");
}

#[test]
fn a_production_member_is_not_written_to_a_plain_file() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    // A production context whose base is Kaiser's: `-o` refuses it before
    // any request.
    let mut config: Config = ctx.config().unwrap();
    config.unset("fhir_base").unwrap();
    ctx.store.save(&config).unwrap();
    let mut state = settings(&ctx);
    state.upsert(kp::session::Context {
        id: PATIENT_ID.into(),
        organization: "kaiser-health-plan".into(),
        label: None,
        relationship: None,
        fhir_user: None,
        signed_in_at: "2020-01-01T00:00:00Z".into(),
    });
    save(&ctx, &state);
    let err = commands::records::run(
        &ctx,
        &RecordsCmd::Export(ExportArgs {
            who: PatientArgs::default(),
            since: None,
            ndjson: false,
            output: Some(dir.path().join("out")),
        }),
    )
    .unwrap_err();
    assert_eq!(err.exit_code(), 2, "{err}");
    let remedy = err.remedy().unwrap_or_default();
    assert!(!err.to_string().contains(PATIENT_ID), "{err}");
    assert!(!remedy.contains(PATIENT_ID), "{remedy}");
    assert!(
        remedy.contains(
            "medrec --profile <FOLDER> import --from kp --from-profile adult1 --patient <WHO>"
        ),
        "{remedy}"
    );
    assert!(mock.requests().is_empty());
}

#[test]
fn a_declined_consent_stores_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    mock.deny_next_authorization();
    let err = commands::auth::run(
        &ctx,
        &AuthCmd::Login(LoginArgs {
            org: "sandbox".into(),
            label: None,
            no_browser: false,
            timeout: 1,
        }),
    )
    .unwrap_err();
    assert_eq!(err.exit_code(), 3, "{err}");
    assert_eq!(
        err.remedy(),
        Some("run `kp auth login --profile adult1 --org sandbox --timeout 1` again")
    );
    assert!(settings(&ctx).grants.is_empty());
    assert!(ctx.sessions(&adult1()).accounts().unwrap().is_empty());
}

#[test]
fn a_second_member_needs_patient_and_logout_removes_one() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");
    // The mock serves one member; a second row stands for a family member
    // signed in through proxy access.
    let mut state = settings(&ctx);
    let mut other = state.grants[0].clone();
    other.id = "pat-0002".into();
    other.label = Some("child1".into());
    state.upsert(other);
    save(&ctx, &state);
    let err = commands::records::run(
        &ctx,
        &RecordsCmd::Export(ExportArgs {
            who: PatientArgs::default(),
            since: None,
            ndjson: false,
            output: Some(dir.path().join("out")),
        }),
    )
    .unwrap_err();
    assert_eq!(err.exit_code(), 2);
    commands::auth::run(
        &ctx,
        &AuthCmd::Logout(LogoutArgs {
            who: PatientArgs {
                patient: Some("adult1".into()),
                org: None,
            },
            forget: false,
        }),
    )
    .unwrap();
    let left = settings(&ctx).grants;
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].label.as_deref(), Some("child1"));
    assert!(ctx
        .vault
        .get(&entry(&format!(
            "smart/kaiser-health-plan-sandbox/{PATIENT_ID}"
        )))
        .unwrap()
        .is_none());
}

#[test]
fn set_credential_then_status_reads_both() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    // The pair goes in as `set-credential` reads it, without setting a
    // variable while other tests run.
    commands::auth::store_pair(
        &ctx,
        Environment::Production,
        false,
        Some(
            ClientCredentials::parse_pair(&pk_cli_secrets::Secret::new("client-one:secret-one"))
                .unwrap(),
        ),
    )
    .unwrap();
    // `info` sees production's without the vault's key.
    let client = commands::registration::client_on_disk(&ctx.vault);
    assert!(client.configured);
    assert_eq!(client.registration.as_deref(), Some("production"));
    let production = credentials::read_client(&ctx.vault, Environment::Production)
        .unwrap()
        .unwrap();
    assert_eq!(production.client_id, "client-one");
    assert_eq!(production.secret().expose(), "secret-one");
    // The sandbox pair `ctx` stored is untouched.
    let sandbox = credentials::read_client(&ctx.vault, Environment::Sandbox)
        .unwrap()
        .unwrap();
    assert_eq!(sandbox.client_id, CLIENT_ID);
    commands::auth::run(&ctx, &AuthCmd::Status).unwrap();
}

#[test]
fn a_production_member_never_comes_from_this_machine() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    let err = commands::auth::run(
        &ctx,
        &AuthCmd::Login(LoginArgs {
            org: "kaiser-health-plan".into(),
            label: None,
            no_browser: true,
            timeout: 1,
        }),
    )
    .unwrap_err();
    assert_eq!(err.exit_code(), 2, "{err}");
    assert!(
        err.to_string().contains("kaiser-health-plan-sandbox"),
        "{err}"
    );
    assert!(mock.requests().is_empty());
}

#[test]
fn a_stored_sign_in_pointing_elsewhere_is_refused_before_any_renewal() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");
    let sessions = ctx.sessions(&adult1());
    let account = format!("smart/kaiser-health-plan-sandbox/{PATIENT_ID}");
    let mut session = sessions.load(&account).unwrap().unwrap();
    session.token_endpoint = "https://mock.example:8080/token".into();
    session.tokens.expires_at = Some(1);
    sessions.save(&session).unwrap();
    let tokens_before = mock
        .requests()
        .iter()
        .filter(|r| r.path() == "/token")
        .count();
    let err = commands::records::export(
        &ctx,
        &ExportArgs {
            who: PatientArgs::default(),
            since: None,
            ndjson: false,
            output: Some(dir.path().join("out")),
        },
    )
    .unwrap_err();
    assert_eq!(err.exit_code(), 2, "{err}");
    let err = commands::api::run(
        &ctx,
        &kp::cli::ApiArgs {
            method: "GET".into(),
            path: "Coverage/cov-0001".into(),
            who: PatientArgs::default(),
        },
    )
    .unwrap_err();
    assert_eq!(err.exit_code(), 2, "{err}");
    let tokens_after = mock
        .requests()
        .iter()
        .filter(|r| r.path() == "/token")
        .count();
    assert_eq!(tokens_after, tokens_before);
}

#[test]
fn no_member_signs_in_at_a_server_that_is_neither_kaiser_nor_this_machine() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    // A base set by hand in the file, past `config set`'s own check.
    let path = ctx.store.path().unwrap();
    std::fs::write(
        &path,
        r#"{"fhir_base":"https://mock.example:8080/fhir","profiles":{"adult1":{}}}"#,
    )
    .unwrap();
    for org in ["kaiser-health-plan", "sandbox"] {
        let err = commands::auth::run(
            &ctx,
            &AuthCmd::Login(LoginArgs {
                org: org.into(),
                label: None,
                no_browser: true,
                timeout: 1,
            }),
        )
        .unwrap_err();
        assert_eq!(err.exit_code(), 2, "{org}: {err}");
        assert!(!err.to_string().contains("synthetic-secret"));
    }
    assert!(mock.requests().is_empty());
}

#[test]
fn one_unreadable_sign_in_does_not_hide_the_others() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");
    let mut state = settings(&ctx);
    let mut other = state.grants[0].clone();
    other.id = "pat-0002".into();
    other.label = Some("child1".into());
    ctx.vault
        .put(
            &entry(&other.account()),
            &pk_cli_secrets::Secret::new("{not json"),
        )
        .unwrap();
    state.upsert(other);
    save(&ctx, &state);
    let (_, status) = commands::auth::status_of(&ctx).unwrap();
    let rows = status["patients"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0]["label"], "adult1");
    assert_eq!(rows[0]["usable"], true);
    assert!(rows[0].get("error").is_none());
    assert_eq!(rows[1]["label"], "child1");
    assert_eq!(rows[1]["usable"], false);
    assert!(rows[1]["error"].as_str().is_some_and(|e| !e.is_empty()));
    assert!(!rows[1]["error"].as_str().unwrap().contains("pat-0002"));
    assert_eq!(
        rows[1]["remedy"],
        "run `kp auth logout --profile adult1 --org sandbox --patient child1`, then `kp auth login --profile adult1 --org sandbox --label child1`"
    );
    assert!(rows[0].get("remedy").is_none());
    assert_eq!(status["authenticated"], true);

    let list = commands::patients::list(&ctx, true, None).unwrap();
    let items = list["items"].as_array().unwrap();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["usable"], true);
    assert_eq!(items[1]["usable"], false);
    assert!(items[1]["error"].is_string());
    assert!(items[1]["refused"].is_null());
    assert_eq!(items[1]["remedy"], rows[1]["remedy"]);
    assert_eq!(items[0]["refused"], false);
    let sandbox = commands::patients::list(&ctx, false, Some("sandbox")).unwrap();
    assert_eq!(sandbox["items"].as_array().unwrap().len(), 2);
    let production = commands::patients::list(&ctx, false, Some("kaiser-health-plan")).unwrap();
    assert_eq!(production["items"], serde_json::json!([]));
}

#[test]
fn logout_forget_keeps_a_broken_old_state_file_and_removes_the_rest() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");
    // A v0.1.x state.json is not read by a plain logout, so a broken one is
    // no obstacle. --forget keeps it, being the only list of v0.1.x's
    // Keychain items, and removes everything else.
    let state_file = ctx.state_file().unwrap();
    std::fs::write(state_file.path(), "{broken").unwrap();
    commands::auth::run(
        &ctx,
        &AuthCmd::Logout(LogoutArgs {
            who: PatientArgs::default(),
            forget: false,
        }),
    )
    .unwrap();
    assert!(state_file.path().exists());
    assert_eq!(ctx.vault.names().unwrap(), ["_client/sandbox"]);
    commands::auth::run(
        &ctx,
        &AuthCmd::Logout(LogoutArgs {
            who: PatientArgs::default(),
            forget: true,
        }),
    )
    .unwrap();
    assert!(state_file.path().exists());
    assert!(credentials::read_client(&ctx.vault, Environment::Sandbox)
        .unwrap()
        .is_none());
    assert!(ctx.vault.names().unwrap().is_empty());
}

#[test]
fn a_sign_in_finishes_when_chrome_does_not_open_and_the_address_is_opened_by_hand() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let mut ctx = ctx(dir.path(), &mock);
    // The window fails; the member opens the printed address themselves,
    // which the follower stands in for, and kp is still listening.
    let opened = window();
    ctx.window = Box::new(move |profile: &ProfileName, url: &str| {
        opened(profile, url)?;
        Err(pk_cli_core::CliError::Other(
            "Chrome is not installed".into(),
        ))
    });
    login(&ctx, "adult1");
    assert_eq!(settings(&ctx).grants.len(), 1);
}

#[test]
fn a_refused_renewal_stops_the_export_and_keeps_the_last_complete_one() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockServer::start();
    let ctx = ctx(dir.path(), &mock);
    login(&ctx, "adult1");
    let out = dir.path().join("out");
    export(&ctx, &out, None);
    let complete = std::fs::read(out.join("records.ndjson")).unwrap();

    // Kaiser refuses the token and then the refresh token: the sign-in is
    // gone, which no retry fixes.
    mock.fail(Fault::unauthorized("/fhir/").times(10));
    mock.fail(
        Fault::new("POST", "/token", 400, r#"{"error":"invalid_grant"}"#)
            .header("Content-Type", "application/json")
            .times(10),
    );
    let failed = commands::records::export(
        &ctx,
        &ExportArgs {
            who: PatientArgs::default(),
            since: None,
            ndjson: false,
            output: Some(out.clone()),
        },
    )
    .unwrap()
    .expect("the export ends on the refusal");
    assert_eq!(failed.exit_code(), 3, "{failed}");
    assert_eq!(
        failed.remedy(),
        Some("run `kp auth login --profile adult1 --org sandbox --label adult1`")
    );
    assert_eq!(std::fs::read(out.join("records.ndjson")).unwrap(), complete);
    let incomplete = std::fs::read_to_string(out.join("records.incomplete.ndjson")).unwrap();
    let trailer: serde_json::Value =
        serde_json::from_str(incomplete.lines().last().unwrap()).unwrap();
    assert_eq!(trailer["complete"], false);

    // The refusal is recorded, so the next use renews before it sends the
    // refused token again.
    let item = ctx
        .vault
        .get(&entry(&format!(
            "smart/kaiser-health-plan-sandbox/{PATIENT_ID}"
        )))
        .unwrap()
        .unwrap();
    let session = pk_med_smart::decode_session(item.expose()).unwrap();
    assert!(session.refused_at.is_some());
}
