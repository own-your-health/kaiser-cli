# Changelog

## v0.3.2 — 2026-11-22

- kp's repository is now `own-your-health/kaiser-cli`, and medical-common,
  medrec-cli and mychart-cli are under `own-your-health` too. kp's
  `self-update`, `info`'s repository, the guide's URL in `kp registration`
  and in the missing-client remedy, and the registration's Application
  Website name the new owner. The medical-common crates come from
  `own-your-health/medical-common` at the same v0.6.9 tag. GitHub redirects
  the old addresses, so v0.3.1 and earlier still update themselves.
- Nothing else changes: the Keychain item (`vault-key` in `piekstra.kp`),
  the vault, the config, the redirect URI and the commands are as they were.
  An app registered with the old Application Website needs no change.
- CI's `security` job runs the gitleaks CLI (8.30.1, its checksum pinned)
  over the whole history, in place of gitleaks-action, which needs a paid
  license on an organization's repository. The job needs only the
  workflow's `contents: read`.

## v0.3.1 — 2026-11-20

- On medical-common v0.6.9 and cli-common v0.33.4, every `pk-cli-*` with
  it: the registration as medical-common revised it (its DESIGN §3.8).
  - `kp registration` prints production's client and
    `kp registration --sandbox` the sandbox's, each with only its own
    redirect URI field, scopes, store command and notes. `--json` gives
    `registration/v2`, with `registration`, `command`, and `doc` as the
    guide's https URL.
  - `info` and `auth status` carry `clients`, a row per environment
    (`registration`, `configured`, `client_type`, `stored_in`). v0.3.0's
    single `client` member is gone. `auth status`'s rows keep the
    `environment`, `stored` and `remedy` they had in v0.3.0, beside the new
    members. A row that could not tell has its `error`: in `auth status`,
    an entry that does not read; in `info`, a vault directory that does
    not. `auth status` prints a line per environment.
  - A missing client's reason is `no_client` (was `no-client`). Its remedy
    names that environment's `kp registration` command and the guide's
    URL. `--json` errors carry `error.reason` and `error.remedy` beside
    `message`.
- `info` checks whether a client is stored with cli-common's
  `Vault::contains`, in place of kp's copy of the vault's file naming. The
  direct `sha2` dependency is gone.

## v0.3.0 — 2026-11-20

- `kp registration` prints what to enter at developer.kp.org, in the
  portal's own labels: the Company/Business Name, the Developer persona,
  the application's name, website and description, the API to request,
  and both redirect URIs on the configured port. It also prints the
  launch, client type, PKCE and scopes, the command that stores each
  environment's client, and what the portal refuses. It is
  medical-common's `Registration`, so it prints as the other medical
  CLIs do, and `--json` gives `registration/v1`. It reads no secret.
- docs/registration.md walks through the portal step by step for someone
  registering for the first time. It covers the sign-up and its 8 to 16
  character password, the phone code, the Developer persona, the
  application, the API request and Kaiser's guide, the sandbox client
  right away, and the production client after approval. It also covers
  what kp uses, how to end access and the terms' main points. The
  README's setup steps moved there, and the README has a quick start from
  registration to `medrec import`, what needs a person, and how long a
  sign-in lasts.
- `info` and `auth status` have a `client` member: whether a client is
  stored and which (production's before the sandbox's), confidential, in
  the vault. `info` reads the vault's file names, never the Keychain, so a
  newly installed kp still asks nothing. `info` lists the `registration`
  capability.
- A command that needs a client and finds none fails with
  medical-common's `missing_client`: exit 2 (it was exit 3), reason
  `no-client`, naming the environment, the guide, `kp registration` and
  the right `kp auth set-credential` command. `auth status`'s client rows
  carry the same remedy; they said `kp auth set-credential` for the
  sandbox too, which needs `--sandbox`.
- On medical-common v0.6.8 (cli-common v0.33.2 as before). Its mock server
  serves a pharmacy fill, which the offline export test now counts.

## v0.2.3 — 2026-11-19

- `records export -o` for a real member: the refusal no longer names an
  unlabelled member's FHIR id, and its remedy is medrec's own command
  (`medrec --profile <FOLDER> import --from kp --from-profile <NAME> --patient <WHO>`)
  or `--ndjson` on stdout. It said to pipe into `medrec import`, which
  reads no standard input.
- A sign-in that ends without one (no answer within `--timeout`, a declined
  consent, or a code Kaiser refuses to exchange) names the command that
  starts it again, with the same profile, organization, label,
  `--no-browser` and `--timeout`:
  ``run `kp auth login --profile adult1 --org sandbox --label pvaluser1` again``.
  An error that already carries a remedy keeps it. Found on Kaiser's
  sandbox, where a sign-in that timed out said only "Sign in again".
- docs/api.md: Kaiser Northwest signs in through Epic with mychart's
  lowercase Start path; its EOB search returned no claims for one member,
  so claims come through `kp`. What developer.kp.org's registration showed.
- Run against Kaiser's sandbox for the first time: two synthetic members
  signed in, one as a proxy, and their exports complete. Kaiser serves no
  `Encounter` or `MedicationDispense` search there (an `unsupported` skip).
  README, CONTRIBUTING (how to run it) and docs/api.md ("Kaiser's sandbox").

## v0.2.2 — 2026-11-17

- On medical-common v0.6.2 and cli-common v0.33.2, every `pk-cli-*` with
  it. The vault's lock file (`.key.lock`, empty) is created owner-only
  (`0600`); one a v0.2.0 or v0.2.1 made `0644` is narrowed the next time
  the lock is taken.
- PRIVACY.md and AGENTS.md: after `auth logout --forget` clears the vault,
  its directory keeps two files that hold no secret, `.key.id` (a random
  id) and `.key.lock` (empty), on purpose (cli-common DESIGN §2.5).

## v0.2.1 — 2026-11-17

- On medical-common v0.6.1 and cli-common v0.33.1, every `pk-cli-*` with
  it. `config.json` is written by cli-common's `ConfigStore::save`, which is
  now owner-only (`0600`) and whole, as kp v0.2.0's own writer was; a
  symlinked config is written through to its target rather than replaced.
- kp's remaining owner-only files (a v0.1.x `state.json`) use cli-common's
  `pk_cli_core::private_file`; kp no longer depends on `pk-cli-drive`.

## v0.2.0 — 2026-11-17

Profiles, one per kp.org account, and every secret in one vault, on
medical-common v0.6.0 and cli-common v0.33.0 (every `pk-cli-*` with it).

- Breaking: `auth login|status|logout`, `patients list`, `records export`
  and `api` need `--profile <NAME>` (or `KP_PROFILE`). There is no default,
  and a lone profile is never picked: exit 2, listing the configured ones.
  Every other command refuses `--profile` (exit 2); `profiles remove` and
  `profiles migrate` take the name as their argument.
  `kp profiles list|add|remove|migrate`; `profiles list --json` is
  `profile-list/v1`, and `profiles remove` asks unless `--force` (exit 6
  when it cannot ask).
- Every secret is in kp's vault (`~/.config/kp/vault/`, encrypted under one
  Keychain item), so a newly built kp asks macOS once rather than once per
  item: the client pair as `_client/production` and `_client/sandbox`, each
  sign-in as `<profile>/<org>/<member>`. A profile's members are its
  settings in the config; `state.json` is no longer written. The config,
  which now holds each member's patient id, is written owner-only (`0600`)
  and whole, as `state.json` was.
- `kp profiles migrate <NAME>` moves what kp v0.1.x kept (one Keychain item
  per client pair and sign-in, listed in `state.json`) into the vault and
  the profile, locally: no network and no new approval.
- The sign-in opens Chrome in the profile's own browser profile, so one
  kp.org account's cookies never sign in another; if Chrome does not open,
  kp prints the address and keeps waiting. `--no-browser` stays.
- Breaking: `auth logout --forget` no longer removes the config: its
  profiles stay (`profiles remove` removes one). Once no other profile
  holds a sign-in, it removes what every profile shares: the client
  credentials, kp v0.1.x's Keychain items with the `state.json` listing
  them, and the vault's key once nothing is left in it. While another
  profile holds one, they stay, and `shared_kept_for` names those profiles.
  A v0.1.x `state.json` that does not read stays, with `legacy_error`, as
  the only list of its items.
- `info` lists the `login-profiles` capability, so medrec passes
  `--profile`. Every remedy names it (`kp auth login --profile adult1 …`).
- Breaking, in the JSON (the schema ids stay `/v1`, as every family CLI's
  do before 1.0):
  - `client-credential/v1` (`auth set-credential`): `keychain_service` and
    `keychain_account` are gone; `vault_entry` names the entry.
  - `auth-logout/v1`: `config_cleared` is gone; `removed` lists vault
    entries, no longer Keychain accounts; new are `profile`,
    `shared_kept_for`, `legacy_removed`, `vault_cleared`, and
    `legacy_error` when it applies.
  - `auth-status/v1`: new are `profile`, and `unmigrated` while a v0.1.x
    `state.json` is there (`sign_ins`, `clients` and `remedy`, or `error`
    and `remedy` when it does not read).
  - New: `profile-add/v1`, `profile-remove/v1`, `profile-migrate/v1`.

## v0.1.7 — 2026-11-16

- On medical-common v0.5.0 and cli-common v0.32.0, every `pk-cli-*` with it.
  The sign-in builds its app with `SmartApp::new`; what it sends to Kaiser
  is unchanged: the loopback redirect `http://127.0.0.1:<port>/callback` on
  the configured `redirect_port`.
- docs/api.md: what kp needs if Kaiser takes only `https` redirect URIs.

## v0.1.6 — 2026-11-16

- A search whose answer says it left results out (an `OperationOutcome`
  issue coded `suppressed`, as Epic sends for a sub-resource the app may not
  read) keeps the results it sent, and the trailer lists one `forbidden`
  skip per type and distinct line, naming the type and no search, as
  medical-common's DESIGN §1.4 and `mychart` do. Before, kp read no outcomes, so such a gap was
  silent.
- docs/api.md: Kaiser's Patient Access API runs on Smile CDR, and its
  CapabilityStatement offers a `patient` search for each type kp searches.
- On medical-common v0.4.9 (cli-common v0.31.1, as before).

## v0.1.5 — 2026-11-16

- A reference on Kaiser's server whose id FHIR does not allow is skipped
  with medical-common's shared code `invalid_reference`, as `mychart` does,
  not `upstream`: it is the server's own data, which asking again does not
  change. Whether it is on Kaiser's server is medical-common's
  `RefusedReference::is_under`.
- An error that ends `records export` after the stream began is printed as
  every other error is, `error: <what> — <remedy>`, not with `;` before the
  remedy, so `medrec import --from kp` finds kp's command either way.
- On medical-common v0.4.8 (cli-common v0.31.1, as before).

## v0.1.4 — 2026-11-16

- `records export` lists a reference on Kaiser's server whose id FHIR does
  not allow as an `upstream` skip in the trailer, naming the type and the
  id's length, never the id. Before, kp left it out without saying so. A
  reference whose `_history` version is not allowed still reads the
  current version.
- A sign-in error says once what to run, as kp spells it: `kp auth login`
  for a missing or refused sign-in, `kp auth logout` then `kp auth login`
  for one that does not read, and `kp self-update` for one a newer kp
  stored. `auth status` and `patients list --status` show that as the
  row's `remedy`.
- On medical-common v0.4.7 (cli-common v0.31.1, as before).

## v0.1.3 — 2026-11-16

- `records export` follows an absolute reference on Kaiser's base however
  the scheme and host are capitalized and whether or not the base ends in
  `/`. Reading references is medical-common's `ReferenceTarget` now, not
  kp's own parser.
- On medical-common v0.4.5 (cli-common v0.31.1, as before).
- PRIVACY.md: kp writes only to files on the member's computer; where the
  program it pipes into (such as `medrec import`) keeps records is that
  program's policy. It no longer calls that store encrypted.

## v0.1.2 — 2026-11-16

- `records export` reads a resource a claim names by an id longer than FHIR
  R4's 64 characters, up to 256, as Epic's unconstrained ids are. Before,
  kp left such a reference out of the export without saying so.
- On medical-common v0.4.4 (cli-common v0.31.1, as before), whose
  `pk_med_fhir::is_id` decides which references are read. Sign-ins stored
  by v0.1.x still read.

## v0.1.1 — 2026-11-12

- With no member signed in, the remedy names the organization asked for:
  `kp records export --org sandbox` (as `medrec import --from kp --org
  sandbox` runs it) now says `kp auth login --org
  kaiser-health-plan-sandbox --label <WHO>`, not the production
  organization.

## v0.1.0 — 2026-11-12

- `orgs list` and `orgs search`: Kaiser Permanente's health plan and its
  sandbox (`kaiser-health-plan`, `kaiser-health-plan-sandbox`) as
  `org-list/v1`.
- `auth set-credential`: the app's Kaiser client id and secret, per
  environment, in the Keychain, from a prompt that does not echo, `--stdin`
  or `--from-env`; refuses to replace a stored one without `--overwrite`.
- `auth login`: SMART App Launch in the system browser (pk-med-smart):
  endpoints from the CapabilityStatement, PKCE S256, the client secret as
  HTTP Basic, one sign-in per member with an optional `--label`.
- `auth status` (`auth-status/v1`, with clients and members) and
  `auth logout [--patient] [--org] [--forget]`.
- `patients list [--org]` (`patient-list/v1`; `--status` adds whether
  each sign-in is usable and whether Kaiser refused it).
- `records export --ndjson | -o <DIR> [--since]`: `health-records/v1`'s
  `records-stream/v1`, for `medrec import --from kp`. A token Kaiser
  refuses is renewed once; refused or failing searches are skips. `-o`
  replaces `records.ndjson` only with a complete stream (an incomplete one
  goes to `records.incomplete.ndjson`), and only for the sandbox.
- A production sign-in goes to Kaiser's server only (https on `kp.org`), a
  sandbox one to Kaiser's or to a server on this machine; `fhir_base` takes
  nothing else.
- A config that still has the retired `environment` key is refused, naming
  `--org kaiser-health-plan-sandbox`, until `kp config unset environment`.
- On medical-common v0.2.0 and cli-common v0.31.1.
- `api GET <PATH>`: one FHIR read as the member.
- `config`: `redirect_port`, `fhir_base` (Kaiser's, or a test server on
  this machine).
- `self-update`, `completions`, `info`.
- `docs/api.md`: how Kaiser's records are reached, clinical (Epic) and
  claims (the Patient Access API), with sources, and what kp pulls.
