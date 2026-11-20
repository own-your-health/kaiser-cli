# kp — Kaiser Permanente health-plan claims

`kp` downloads your own Kaiser Permanente health-plan claims (Explanation of
Benefit) and coverage through Kaiser's Patient Access API, the CMS
interoperability API Kaiser publishes at
[developer.kp.org](https://developer.kp.org). It runs on your computer and
sends nothing anywhere else ([PRIVACY.md](PRIVACY.md)).

Unofficial; not affiliated with or endorsed by Kaiser Permanente.

It is for any Kaiser Permanente member with a kp.org account, in any
region, who wants their own claims and coverage on their own computer, and
those of the family members they act for on kp.org. Clinical records
(visits, results, notes) at Kaiser come through Epic: use
[`mychart`](https://github.com/piekstra/mychart-cli) for those.
[docs/api.md](docs/api.md) explains the split.

## Status

v0.3.0: tested against medical-common's mock server and run against
Kaiser's sandbox (synthetic members, a proxy among them;
[docs/api.md](docs/api.md#kaisers-sandbox)). Production access needs
Kaiser's approval of your app, and has not been run yet.

## Install

Pick the target (`aarch64-apple-darwin` for Apple Silicon,
`x86_64-apple-darwin` for an Intel Mac, `x86_64-unknown-linux-gnu`), check
the archive against its `.sha256`, then put `kp` on your PATH:

```sh
t=aarch64-apple-darwin
u=https://github.com/piekstra/kaiser-cli/releases/latest/download/kp-$t.tar.gz
curl -sSLO "$u" && curl -sSLO "$u.sha256"
shasum -a 256 -c "kp-$t.tar.gz.sha256"    # on Linux: sha256sum -c
tar xzf "kp-$t.tar.gz" && mkdir -p ~/.local/bin && mv kp ~/.local/bin/
kp self-update                            # later releases
```

From source: `make install`.

## Quick start

1. Register an app at developer.kp.org, once:
   [docs/registration.md](docs/registration.md) goes through it step by
   step, and `kp registration` prints what to enter. Kaiser's terms keep
   the client id and secret it issues confidential, so each household
   registers its own. The sandbox works right away; production waits on
   Kaiser's review, answered by email.
2. Store the client id and secret it gives you:

   ```sh
   kp auth set-credential --sandbox    # the sandbox's; asks for each without echoing
   kp auth set-credential              # production's, once Kaiser approves
   ```

3. Add a profile for each kp.org account, and sign its member in:

   ```sh
   kp profiles add adult1
   kp auth login --profile adult1 --label adult1
   ```

   Chrome opens Kaiser's sign-in in adult1's own browser profile. Sign in
   with the kp.org user ID and password, pick the member, and approve
   sharing; the window closes when kp has the sign-in. A member who acts
   for family members on kp.org signs in once for each, picking them on
   Kaiser's page, each with a label (`--label child1`). `--org sandbox`
   signs in one of Kaiser's synthetic test members instead.
4. Download the records, or let medrec file them:

   ```sh
   kp records export --profile adult1 --patient adult1 --ndjson    # records-stream/v1 on stdout
   medrec --profile <FOLDER> import --from kp --from-profile adult1 --patient adult1 --person <WHO>
   ```

   [medrec](https://github.com/piekstra/medrec-cli) v0.2.2 or later runs
   `kp records export` itself. Its own `--profile` names the records folder,
   and `--from-profile` names kp's profile. Later imports leave out
   `--person`: medrec remembers whose they are.

### What needs you

| Step | What you do |
|---|---|
| Registration | The email and phone codes of the developer account; copying each secret, which the portal shows once |
| Production | Wait for Kaiser's approval email |
| Each sign-in | kp.org's user ID and password, any code kp.org sends, the choice of member, and the approval on Kaiser's consent page. kp never fills Kaiser's pages |
| A new kp binary on macOS | One Keychain dialog, on the first run after an install or update: choose Always Allow |

### How long a sign-in lasts

In Kaiser's sandbox the member's consent lasts a year and an access token
an hour. kp renews the token with the refresh token on its own, so exports
need no sign-in until the consent ends, the member removes kp on kp.org
(Profile and Preferences, Data sharing & authorization, Manage access to
health plan information), or Kaiser refuses the renewal. Then the error
names the `kp auth login` command to run. Kaiser's guide does not give
production's lifetimes.

## Use

```sh
export KP_PROFILE=adult1                          # or --profile adult1 on each
kp patients list                                  # who is signed in
kp records export --ndjson --patient adult1       # records-stream/v1 on stdout
kp records export --ndjson --since 2026-01-01     # only what changed since
kp api GET 'ExplanationOfBenefit?patient=<id>'    # one read, the raw answer
```

Each profile signs in in its own Chrome profile, so one account's kp.org
cookies never sign in another. `KP_PROFILE` stands in for `--profile`; there
is no default.

Coming from kp v0.1.x, `kp profiles migrate <NAME>` moves the sign-ins and
client credentials it kept, one Keychain item each, into the vault and the
profile: no network and no new approval.

`records export` pulls every claim (`ExplanationOfBenefit`), coverage,
encounter and pharmacy dispense Kaiser serves for the member, then the
patient, providers, organizations and locations they name. It writes
medical-common's `records-stream/v1`. A search or reference read Kaiser
refuses or fails, a result a search's answer says it left out, and a
reference that names no valid FHIR id, are listed in the stream's trailer
as skipped; the export stops only when the sign-in, the
network or the output fails. Every sign-in error ends with the `kp` command
that fixes it.

## Commands

| Command | Does |
|---|---|
| `kp orgs list`, `kp orgs search <TEXT>` | the health plan and its sandbox (`org-list/v1`) |
| `kp profiles list` | the profiles, one per kp.org account (`profile-list/v1`) |
| `kp profiles add <NAME>` | add one: 1 to 32 lowercase letters, digits, `-` or `_` |
| `kp profiles remove <NAME> [--force]` | its sign-ins, its browser profile and its config; asks first |
| `kp profiles migrate <NAME>` | kp v0.1.x's sign-ins and client credentials into the vault and the profile |
| `kp registration` | what to enter at developer.kp.org, on the configured redirect port (`registration/v1`) |
| `kp auth set-credential [--sandbox] [--overwrite]` | store the app's client id and secret (shared by every profile) |
| `kp auth login --profile <NAME> [--org sandbox] [--label <WHO>] [--no-browser] [--timeout <MIN>]` | sign a member in; waits up to `--timeout` minutes (default 10, at most 60) |
| `kp auth status --profile <NAME>` | what is stored (`auth-status/v1`), per environment and member; `client` says whether a client is stored, and which (production's before the sandbox's), and a client row that is not stored carries the `remedy`; a sign-in that does not read has its `error` and `remedy`; `unmigrated` counts what kp v0.1.x left (`kp profiles migrate <NAME>` moves it), or has the `error` and `remedy` of a v0.1.x `state.json` that does not read |
| `kp auth logout --profile <NAME> [--patient <WHO>] [--org <ORG>] [--forget]` | remove the profile's sign-ins; `--forget`, once no other profile holds one, also what they share: the client credentials, kp v0.1.x's Keychain items, and the vault's key |
| `kp patients list --profile <NAME> [--status] [--org <ORG>]` | the profile's members (`patient-list/v1`); `--status` reads each sign-in: `usable`, `refused`, and `error` and `remedy` for one that does not read |
| `kp records export --profile <NAME> --ndjson\|-o <DIR> [--since <DATE>]` | the member's records as `records-stream/v1`; `-o` for the sandbox only |
| `kp api --profile <NAME> GET <PATH>` | one FHIR read as the member |
| `kp config path\|show\|set\|unset` | `redirect_port` (default 7469), `fhir_base` (Kaiser's, or a test server on this machine for the sandbox) |
| `kp self-update`, `kp completions <SHELL>`, `kp info` | the family's standard commands; `info`'s `client` says whether a client is stored, and which, without the Keychain |

Every command takes `--json`, except `records export --ndjson`, whose
output is already the NDJSON stream. Exit codes: 0 ok, 2 usage or no client
stored (reason `no-client`), 3 auth, 4 not found, 5 upstream, 6 confirmation
required.

## Build

```sh
make install    # cargo install --locked, then signs the binary (macOS)
make verify     # what CI runs
```

Licensed under MIT or Apache-2.0.
