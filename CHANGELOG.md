# Changelog

## v0.1.6 — 2026-11-16

- A search whose answer says it left results out (an `OperationOutcome`
  issue coded `suppressed`, as Epic sends for a sub-resource the app may not
  read) keeps the results it sent and lists a `forbidden` skip per distinct
  line in the trailer. Before, kp read no outcomes, so such a gap was
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
