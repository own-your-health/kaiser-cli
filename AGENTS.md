# AGENTS.md — kaiser-cli

`kp` reads a Kaiser Permanente member's own health-plan claims
(Explanation of Benefit) and coverage through Kaiser's Patient Access API.
It conforms to piekstra-cli/1 and medical-common's `health-records/v1`
profile, on the `pk-cli-*` crates from
[cli-common](https://github.com/piekstra/cli-common) and the `pk-med-*`
crates from [medical-common](https://github.com/piekstra/medical-common).

Clinical records at Kaiser come through Epic, so they are `mychart`'s, not
kp's (`docs/api.md`, Route).

## Layout

| File | Owns |
|---|---|
| `src/main.rs` | parsing, dispatch, `info`, exit codes |
| `src/cli.rs` | the clap tree |
| `src/commands/*.rs` | one module per command group (`profiles.rs`: `list`, `add`, `remove`, `migrate`; `registration.rs`: the portal's values as medical-common's `Registration`, the missing-client error, the `client` member of `info` and `auth status`); `Ctx` in `mod.rs`: the config and its owner-only writes, the `--profile` resolution, the vault and the v0.1.x Keychain, the refresh lock, the sign-in window and `forget_browser` |
| `src/kaiser.rs` | the two organizations (the health plan and its sandbox): FHIR base, regions, scopes, `--org` resolution and search, `KaiserQuirks` |
| `src/session.rs` | a signed-in member (`Context`, a profile's grant), picking one, the live token with renewal over the profile's sessions in the vault (`ProfileSessions`, `Live`) |
| `src/export.rs` | what `records export` pulls, in order, and which failures skip rather than stop |
| `src/config.rs` | the config: redirect port, FHIR base override, the profiles and each one's grants (`ProfileSettings`) |
| `src/credentials.rs` | the app's client id and secret: the vault entries `_client/<environment>`, whether one is on disk (`client_on_disk`, no Keychain), and v0.1.x's account names |
| `src/state.rs` | kp v0.1.x's `state.json`, the list of its Keychain items, which `profiles migrate` and `auth logout --forget` read and remove |
| `src/fsutil.rs` | owner-only files written whole |
| `examples/mock_kaiser.rs` | `make mock`: medical-common's mock SMART + FHIR server, for dogfooding |
| `docs/api.md` | Kaiser's API, quoted, with sources and traps; read before changing auth or export |
| `docs/registration.md` | developer.kp.org step by step, for someone registering an app for the first time; `kp registration` prints its values, so change the two together |

## Medical data

Medical records are the most sensitive data a household has.

- The repository carries no personal data: invented test data only, checked
  by `tests/no_personal_data.rs` and gitleaks.
- Develop and dogfood against medical-common's mock server or Kaiser's
  sandbox (its test members are synthetic). A check against real data
  reports counts, statuses and exit codes only, or is left to the owner in
  their own terminal. An agent never reads, prints or summarizes a real
  record, document or member name.
- No MCP surface: kp is not registered in cli-mcp and ships no `mcp`
  command, because a tool call would hand records to the model making it
  (the effort's plan, §8, until the owner decides otherwise).

## Credentials

- Profiles: one per kp.org account (`adult1`), in the config only. Every
  command that uses a sign-in (`auth login|status|logout`, `patients`,
  `records`, `api`) needs `--profile <NAME>` or `KP_PROFILE`; there is no
  default, and a lone profile is never picked (exit 2, listing them).
  `orgs`, `profiles`, `config`, `auth set-credential` and `info` take none.
  A profile's grants (organization, member id, label) are its settings in
  the config.
- Every secret is in kp's vault (cli-common `pk_cli_auth::vault`):
  encrypted files in `~/.config/kp/vault/` under one Keychain item, the key
  `vault-key` in service `piekstra.kp`. So a newly built kp asks macOS once,
  not once per sign-in. Install and upgrade kp only when the owner is
  present, and leave the first run after it to them: that run waits on the
  one dialog. `auth logout --forget` deletes the key item; the vault's
  directory keeps `.key.id` (a random id) and `.key.lock` (empty), both
  `0600`, on purpose (cli-common DESIGN §2.5).
- Kaiser registers each app as a confidential client. Its client id and
  secret are the vault entries `_client/production` and `_client/sandbox`
  (a leading `_` is no profile's), stored by `kp auth set-credential` from a
  prompt that does not echo, `--stdin` or `--from-env`. Kaiser's terms keep
  both confidential: never printed, logged, or put in argv. Each household
  registers its own app (`docs/registration.md`); no client id ships with
  kp, and none belongs in the repository.
- `info`'s `client` member (medical-common DESIGN §3.8) says whether a
  client is stored, production's before the sandbox's, from the vault's
  file names alone (`credentials::client_on_disk`), so `info` never reads
  the Keychain; `auth status` reads the entries, and has each
  environment's row. A missing client is `pk_med_smart::missing_client`:
  exit 2, reason `no-client`, naming the environment, the guide,
  `kp registration` and the `set-credential` command
  (`registration::missing`).
- A member's sign-in is pk-med-smart's session, the vault entry
  `<profile>/<org>/<FHIR patient id>` (`ProfileSessions`). Errors never name
  it, since it holds the member's id. Renewal runs under the family's
  refresh lock, so two `kp` runs never spend one refresh token twice.
- The sign-in opens Chrome in the profile's own browser profile
  (`SignInWindow`, `browser-profile-<profile>`), so one kp.org account's
  cookies never sign in another; `profiles remove` deletes it. kp never
  fills Kaiser's sign-in or consent page: kp.org's terms forbid automated
  access (docs/api.md).
- kp v0.1.x kept one Keychain item per secret and a `state.json`;
  `kp profiles migrate <NAME>` moves them into the vault and the profile,
  locally, each old item costing one dialog, once.
- Tests and `make verify` never read the real Keychain:
  `KP_KEYCHAIN_SERVICE` names an empty service for the surface tests and
  `smoke`, with the vault beside their scratch config; the in-process
  tests give the vault cli-common's `MemoryKeys` and a temporary
  directory.
- If a `kp` call hangs, it waits on a Keychain approval. Stop calling it and
  tell the owner; a retry only raises another dialog.

## Conventions

- `--json` on every command, one schema-tagged document on stdout, except
  `records export --ndjson`, whose stream is the output.
- Arguments, and secrets from `--stdin` or `--from-env`, are checked before
  the Keychain or the network.
- Read-only: kp never writes to Kaiser; `api` takes GET only.
- `records export -o` writes only the sandbox's records to a file, and
  refuses before the Keychain otherwise: a real member's records go to
  stdout, which medrec reads when `medrec import --from kp` runs kp. Errors
  name a member by label or `<WHO>`, never by FHIR id.
- Where a sign-in may go is an allowlist (`kaiser::require_server`, on the
  parsed host): `kaiser-health-plan` to Kaiser's server only (https on
  `kp.org`), `kaiser-health-plan-sandbox` to Kaiser's or to a server on
  this machine (the mock). `config set fhir_base` refuses any other host,
  and `auth login`, `records export` and `api` check again. So the client
  secret never reaches another host, and a synthetic or foreign member
  never carries the production organization id, by which medrec files
  records.
- Every binary-producing `make` target signs with the stable
  `pk-cli-codesign` identity.

## Tests

`make verify` (fmt, clippy, tests, smoke), offline:

- `tests/cli_surface.rs`: the binary's help, exit codes, JSON errors,
  `info` (with `login-profiles`, `registration` and `client`),
  `registration` (`registration/v1` on the configured port), completions,
  `orgs`, `config`,
  `profiles`, a login command refused without a configured `--profile`,
  logged-out `auth status` (`client`, each environment's remedy).
- `tests/e2e.rs`: in process against medical-common's mock server on
  127.0.0.1, with a stand-in browser and a vault over `MemoryKeys`, as
  profile `adult1`: sign-in (discovery
  from the CapabilityStatement, HTTP Basic at the token endpoint, the
  scopes), export counts, `--since`, a refused token renewed once, refused
  and failing searches as skips, a refused renewal (exit 3 with kp's
  remedy, `refused_at` recorded, the last complete export kept), a
  declined consent, `-o` refused for a production member, two members and
  logout, `set-credential`, a reference whose id is longer than 64
  characters and one FHIR does not allow (`tests/fixtures/`, invented), and
  kp's remedy for a missing, unreadable or newer-format sign-in, and a
  search whose answer says it left results out (`MockServer::withhold`),
  a sign-in that finishes after Chrome fails to open, and `--forget` past
  a v0.1.x `state.json` that does not read.
- `tests/no_personal_data.rs`: personal-data shapes in tracked files.
- Unit tests in each module.

## Definition of done

`make verify` and CI green, the installed binary dogfooded against the mock
and Kaiser's sandbox, `docs/api.md` matching what Kaiser does, and no
personal data anywhere in the diff.
