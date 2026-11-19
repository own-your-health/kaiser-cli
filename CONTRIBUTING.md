# Contributing

- Read `AGENTS.md` (layout, rules for medical data and credentials) and
  `docs/api.md` (Kaiser's API, its sources and its traps) first.
- `make verify` runs what CI runs: fmt, clippy with `-D warnings`, the tests
  and the smoke test on the release binary. It needs no network beyond the
  first dependency fetch and never reads the real Keychain.
- Dogfood the installed binary against `make mock`, medical-common's mock
  SMART + FHIR server serving one synthetic member, before Kaiser's sandbox.
  It prints its FHIR base and the synthetic client pair:

  ```sh
  make install && make mock     # in another terminal
  export KP_KEYCHAIN_SERVICE=piekstra.kp.dogfood KP_PROFILE=dogfood
  kp config set fhir_base <the mock's FHIR base>
  kp profiles add dogfood
  printf '%s' '<the printed pair>' | kp auth set-credential --sandbox --stdin
  kp auth login --org sandbox --no-browser   # then open the printed address
  kp records export -o /tmp/kp-mock
  kp auth logout --forget && kp profiles remove dogfood --force
  ```

  With `KP_KEYCHAIN_SERVICE` set, the vault lives beside the config
  (`vault.<service>/`), and `auth logout --forget` removes its key item
  once nothing is left in it.

- Then Kaiser's sandbox, with `fhir_base` unset and the app's sandbox pair
  stored (`kp auth set-credential --sandbox`):

  ```sh
  kp profiles add sandbox
  kp auth login --profile sandbox --org sandbox --label pvaluser1
  kp records export --profile sandbox --patient pvaluser1 -o /tmp/kp-sandbox
  ```

  Sign in as one of Kaiser's synthetic test members (`Pvaluser1` to
  `Pvaluser10`; their passwords are in the Test Accounts table of the
  Patient Access API's EOB Implementation Guide on developer.kp.org), then
  accept on Kaiser's consent page. `Pvaluser3` acts for three family
  members: sign in once for each, picking them on the consent page, each
  with its own `--label`.

- Test data is invented. Never commit a real response, record or export;
  `tests/no_personal_data.rs` and gitleaks check every change.
- Every change lands through a pull request with a review and green checks.
- Dates on the command line are `YYYY-MM-DD`. Every command takes
  `--json`, except `records export --ndjson`, whose stream is the output.
