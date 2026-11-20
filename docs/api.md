# Kaiser Permanente: how records are reached

Kaiser Permanente offers patients two separate FHIR APIs, run by different
systems, registered in different places, and consented to separately by the
member.

| | Clinical records | Health-plan claims |
|---|---|---|
| What | US Core / USCDI clinical data: conditions, results, medications, allergies, immunizations, encounters, notes and documents, plus `Coverage`; `ExplanationOfBenefit` declared; at Northwest its search returned none for one member ([The result](#the-result)) | CARIN Blue Button `ExplanationOfBenefit` with `Coverage`, `Patient`, `Practitioner`, `Organization`, `Location`; dates of service from 2016-01-01 |
| Server | Epic (February 2026), one per region, under `fhir.kp.org` | Smile CDR behind Apigee, one for all regions, under `kpx-service-bus.kp.org` |
| Registration | Epic's, at fhir.epic.com (open.epic). Kaiser: "No approval (beyond the OAuth security flow) is needed for the open.epic API" | Kaiser's own, at developer.kp.org: a "Secure API" that "require[s] an account and approval" |
| Client | Whatever Epic issues (the `mychart` app: public client, PKCE, Epic dynamic client registration) | Confidential: client id and secret, `client_secret_basic`, PKCE S256 required |
| Testing | "There is no testing with Kaiser. All testing is done with Epic in their Testing Sandbox" | Kaiser sandbox: same hosts, `sandbox` scope, 10 published synthetic test members |
| CLI | `mychart --org "Kaiser Permanente - <region>"` | `kp`, only if the household registers at Kaiser |

## Route

1. **Clinical records go through `mychart`.** Kaiser publishes no clinical API
   of its own; its developer portal lists "KP Interoperability Clinical Data"
   as "Other Interoperability": "For Clinical Data exchange Kaiser Permanente
   use Epic on FHIR. Any application can connect to Kaiser Endpoints to
   exchange CCDS or USCDI - data classes and elements. The app developer must
   provide connectivity details about the application to epic." Each Kaiser
   region is one Epic organization in Epic's directory, so it is an `--org` of
   `mychart`, not a separate CLI.
2. **Claims (EOB) come through `kp`.** The
   health plan's Patient Access API is "separate and distinct from the
   open.epic clinical data API" and needs a reviewed registration at Kaiser.
   If the household registers, `kp` implements `health-records/v1` over it so
   `medrec import --from kp` works unchanged. Without that registration,
   Kaiser clinical records and `Coverage` still arrive through `mychart`.
   Kaiser publishes members' claims for apps through the Patient Access
   API. Kaiser's Epic declares the EOB API too, but at Northwest its search
   returned none for one member with the Claim sub-type allowed (2026-11-19,
   [The result](#the-result)). Whether Epic holds none of Kaiser's claims or
   that member has none shows only beside `kp`'s count. `mychart` v0.2.1 and
   later still search for them, and a refused search is a skip.

Epic distributes a patient-facing USCDI app's client id to every community
member "on a rolling 12-hour cycle" unless the member "has disabled
auto-download functionality"; Kaiser's documentation does not say whether it
has. One GET of a Kaiser region's authorize URL answers it without signing
in, with `response_type=code`, the production `client_id` and the https
redirect mychart registered for production
(`https://piekstra.github.io/mychart-cli/callback`), not following
redirects (`mychart auth probe --org <id>` makes it). A known client is
redirected to the page that begins Epic's sign-in,
`…/Authentication/OAuth/Start`, with status 302 and no `error=` in the
query. An unknown one gets a 200 "OAuth2 Error" page and no redirect (a
dummy client id at Northwest), or a redirect with `error=`. Never print the
client id while running it. On 2026-11-16 `mychart`'s production client id
was known at Northwest, Mid-Atlantic, Georgia, Northern California and
Southern California, and not yet at Colorado, Hawaii and Washington.

At Northwest, Kaiser's Epic returned no `ExplanationOfBenefit` to a member's
token on 2026-11-19 ([The result](#the-result)).

## Clinical: Kaiser's Epic endpoints

Epic's User-access Brands bundle (`https://open.epic.com/Endpoints/Brands`,
97 MB on 2026-11-12, about 135 s to download) lists 8 Kaiser brands. Use it,
not the deprecated R4 Endpoint bundle, which still lists retired bases.

| Brand (Epic brand id) | FHIR R4 base in the Brands bundle | Authorization server |
|---|---|---|
| Kaiser Permanente - Colorado (478) | `https://fhir.kp.org/service/ptnt_care/EpicEdiFhirRoutingSvc/v2014/esb-envlbl/140/api/FHIR/R4/` | `.../KPPolarisPortal/esb-envlbl/140/oauth2` |
| Kaiser Permanente - Georgia (237) | `.../esb-envlbl/200/api/FHIR/R4/` | `.../esb-envlbl/200/oauth2` |
| Kaiser Permanente - Hawaii (479) | `.../esb-envlbl/130/api/FHIR/R4/` | `.../esb-envlbl/130/oauth2` |
| Kaiser Permanente - Mid-Atlantic (480) | `https://FHIR.KP.ORG/.../esb-envlbl/170/api/FHIR/R4` | `.../esb-envlbl/170/oauth2` |
| Kaiser Permanente - Northern California (482) | `https://FHIR.KP.ORG/.../esb-envlbl/326/api/FHIR/R4/` | `.../esb-envlbl/326/oauth2` |
| Kaiser Permanente - Northwest (132) | `https://FHIR.KP.ORG/.../esb-envlbl/190/api/FHIR/R4/` | `.../esb-envlbl/190/oauth2` |
| Kaiser Permanente - Southern California (483) | `.../esb-envlbl/226/api/FHIR/R4/` | `.../esb-envlbl/226/oauth2` |
| Kaiser Permanente - Washington (439) | `https://fhir.kp.org/Interconnect-FHIR-PRD/api/FHIR/R4` | `.../esb-envlbl/750/oauth2` |

`...` is `https://fhir.kp.org/service/ptnt_care/EpicEdiFhirRoutingSvc/v2014`
for bases and `https://fhir.kp.org/KPPolarisPortal` for authorization servers.

Every base's `.well-known/smart-configuration` (public, read 2026-11-12) is
identical except for the endpoint URLs:

- `capabilities`: `launch-standalone`, `client-public`,
  `client-confidential-symmetric`, `client-confidential-asymmetric`,
  `context-standalone-patient`, `permission-offline`, `permission-patient`,
  `permission-v1`, `permission-v2`, `sso-openid-connect`, `authorize-post`
  (plus EHR-launch capabilities).
- `code_challenge_methods_supported`: `S256`.
- `grant_types_supported`: `authorization_code`, `refresh_token`,
  `client_credentials`, `urn:ietf:params:oauth:grant-type:jwt-bearer`,
  `urn:ietf:params:oauth:grant-type:token-exchange`.
- `token_endpoint_auth_methods_supported`: `client_secret_post`,
  `client_secret_basic`, `private_key_jwt`.
- One `jwks_uri` key set for all regions (`.../Security/Open/PublicKeys/530027/OIDC`).

The CapabilityStatement (`metadata`) lists 59 resource types, including
`Coverage`, `ExplanationOfBenefit`, `Claim`, `DocumentReference` and `Binary`.
That is Epic's statement for the version (February 2026), not evidence that a
region holds data of each type.

### Traps

- **Host case.** Three bases use `FHIR.KP.ORG`. Compare and key Keychain items
  on a lower-cased scheme and host; send `aud` exactly as the directory spells
  the base until a sign-in shows Epic accepts another spelling.
- **Trailing slash.** Mid-Atlantic and Washington bases have none; the others
  do. Join paths without doubling or dropping the slash.
- **Authorization server is not under the base.** It is on another path
  (`KPPolarisPortal`) and, for the retired bases, another number: the
  deprecated R4 bundle's Southern California base `.../212/...` authorizes at
  226, and Northern California `.../312/...` (plus a second entry,
  "Kaiser – NCAL - prod 326 (CIS)") at 326. Always discover; never derive
  OAuth URLs from the base.
- **`implementation.url` differs from the base.** The CapabilityStatement names
  `https://fhir.kp.org/KPPolarisPortal/esb-envlbl/<n>/api/FHIR/R4`. Use the
  directory's base for requests and `aud`.
- **Epic's two lists of USCDI APIs disagree on EOB.** Epic's API catalog
  marks `ExplanationOfBenefit.Read (Claim) (R4)` and
  `ExplanationOfBenefit.Search (Claim) (R4)` as USCDI, and Epic's app
  registration form follows it: an app that selects them stays eligible for
  USCDI v3 automatic distribution (seen on `mychart`'s registration,
  2026-11-16). The appendix of Epic's "Patient-Facing Apps Using FHIR" page
  lists no `ExplanationOfBenefit` among the USCDI v3 APIs, and offers EOB
  through a separate "CMS Payer Apps" route instead. The catalog is what
  registration checks, so it wins
  ([Claims through Epic](#claims-through-epic)).

### Proxy access to a child's record

Kaiser's "Act for a Family Member" gives a parent kp.org access to a dependent
under 18: from the parent's own kp.org account, with the child's name, date of
birth and medical record number; Kaiser mails a security code to the child's
home address, or Member Services sets it up in person. Whether Epic's OAuth at
Kaiser then offers the child in the patient picker is Epic behaviour, verified
in Epic's sandbox by the `mychart` session and at Kaiser on the first proxy
sign-in.

### Regions are separate sign-ins

Each brand is its own Epic organization with its own authorization server, so
a person with records in two regions signs in twice. Northwest (Oregon and
SW Washington, base 190) and Washington (the former Group Health, base
`Interconnect-FHIR-PRD`, authorization server 750) are different
organizations even though both serve Washington state. Northwest's member
portal is Epic MyChart (`https://healthy.kaiserpermanente.org/mychartnw/`).

### Sign-in goes through kp.org

Kaiser's MyChart has no login page of its own. Epic's authorize step hands
the member to MyChart's Start page, and Kaiser's Start page fails without a
kp.org session. Checked at Northwest on 2026-11-17, without credentials
except the first row, which a browser sign-in reached:

| Request | Answer |
|---|---|
| `https://fhir.kp.org/KPPolarisPortal/esb-envlbl/190/oauth2/authorize`, a client id Epic knows | 302 to `https://healthy.kaiserpermanente.org/fhirnw/Authentication/OAuth/Start` |
| The same, a dummy client id | 200, "OAuth2 Error" |
| `/fhirnw/Authentication/OAuth/Start`, with or without OAuth parameters, no kp.org session | 500, Kaiser's "Page Not Found" page |
| `/fhirnw/Authentication/Login`, MyChart's own login | 410 |
| A path that does not exist | 410 |

Epic's sandbox serves MyChart's login form at that same Start page when no
session exists. At Kaiser a browser that went straight to authorize ended on
the "Page Not Found" page, so the member signs in at kp.org first, in the
same browser: `mychart` v0.5.0 opens its sign-in window with kp.org's
sign-on page first, then the authorize address, in the profile's own Chrome
profile. Every region authorizes under `KPPolarisPortal`, so the others
probably behave the same.

That order fails too at the capitalized Start path (tried 2026-11-19,
Kaiser Permanente - Northwest, mychart v0.5.0 and its production client id):

| Step | Result |
|---|---|
| The member signs in at kp.org in the sign-in window, with the one-time code | signed in: the window's other tab is at `/mychartnw/Home` |
| The authorize tab opened with the window, before that sign-in | `/fhirnw/Authentication/OAuth/Start`, titled "Kaiser Permanente - Page Not Found" |
| The same authorize address again, in a new tab of that window, with the kp.org session live | redirected again to that Start page, with the same title |

The sign-in timed out with nothing stored. The member's MyChart session lives
under `/mychartnw/`, while Epic's OAuth pages are under `/fhirnw/`, whose own
login answers 410.

The same path in lowercase, `/<region>/authentication/oauth/start` with the
same query, is reported to reach Kaiser's sign-in and complete (another
project's sign-ins in Northern and Southern California; see Sources). Without
cookies, Northwest's lowercase page with the query answers a redirect to
`/fhirnw/Home/Logout`, Epic's first sign-in step, where the capitalized one
answers 500 (mychart's check, 2026-11-19). `mychart` v0.5.1 sends the
authorize request itself, and when the answer is the capitalized Start page
and that page answers 500, its window opens the lowercase path with the same
query, after kp.org's sign-on page. That works at Northwest (2026-11-19,
mychart v0.5.1, production client id):

| Step | Result |
|---|---|
| The window opens, before any sign-in | both tabs at `identityauth.kaiserpermanente.org` (`/as/authorization.oauth2` and `/idp/SSO.saml2`), titled "Sign in \| Kaiser Permanente" |
| The member signs in once, with the one-time code, picks the longest sharing period and approves | `mychart auth login` exits 0, the sign-in stored, verified and renewable |

The member signed in once. kp.org's tab is a duplicate: without cookies,
the lowercase Start page goes through Epic's `Home/Logout`,
`InsideAsp/Mode`, `Authentication/Login` and `Saml/Login` to Kaiser's own
sign-in at `identityauth.kaiserpermanente.org/idp/SSO.saml2` (mychart's
trace, 2026-11-19), so `mychart` v0.5.2 opens only the authorize address. No other entry point was tried: Kaiser's developer terms forbid
testing its APIs without written approval.

kp.org's own sign-in:

- `https://healthy.kaiserpermanente.org/sign-on` redirects to
  `https://identityauth.kaiserpermanente.org/as/authorization.oauth2`
  (PingFederate, behind PingAccess and an F5 BIG-IP). It is a plain HTML
  form: `pf.username` and `pf.pass`, posted to
  `/as/<flow>/resume/as/authorization.ping`.
- Every sign-in sends a one-time passcode to the email or mobile number in
  the member's notification preferences. There is no authenticator (TOTP)
  option.
- The page fingerprints the device for PingOne Risk Management
  (fingerprintjs2, written to `pingone.risk.device.profile*` cookies).
  reCAPTCHA is in the page but switched off.
- kp's Patient Access authorize step redirects to a second PingFederate
  ([Claims: Kaiser's Patient Access API](#claims-kaisers-patient-access-api)).
- kp.org's Terms & Conditions (last updated September 2026, version 2)
  prohibit using any "robot", "spider" or "other automatic device, program,
  algorithm or methodology, to (1) access, acquire, copy or monitor any
  portion of the Site". For a violation, "Kaiser Permanente expressly
  reserves the right, in its sole discretion, to terminate a user's access"
  to the Site. `kp` opens a browser window for the sign-in and never fills
  the form: the member signs in and approves.

## Imaging

| Route | What it gives | DICOM |
|---|---|---|
| Epic FHIR (`mychart`) | Radiology reports as `DiagnosticReport` and clinical notes (Kaiser's guide lists "Imaging Narrative" among the `DocumentReference` note types); `Media (Study)` + `Binary (Study)` images | No |
| Kaiser claims API | Nothing about images | No |
| Release of information, per region | Copies of the studies | Yes |

- **`ImagingStudy` cannot be requested.** Kaiser's CapabilityStatements list
  `ImagingStudy` (read and search, including an `endpoint` parameter), but
  Epic's API catalog (fhir.epic.com Specifications) offers no `ImagingStudy`
  API to register, and it is not on the USCDI v3 automatic-distribution list.
- **`Media (Study)` is not DICOM.** Epic: "The FHIR Media resource defines
  detailed information about images that are acquired or used in healthcare,
  specifically cardiology and endoscopy." Each `Media` points to a
  `Binary (Study)` holding a PNG or JPEG. Both are on the USCDI v3 list.
- **Washington.** "Copies of digital images are available on CD";
  "digital images aren't available for members to view online at this time".
  Request with the Authorization to Release Health Care Information form
  ("Radiology images (on CD)", sent to the Central Imaging Center) or by
  calling the imaging center. Processing is "the same or next day" to "more
  than 72 hours". Free when sent to a non-Kaiser facility for ongoing care;
  "Charges may vary for personal copies". "Any CD not handed directly to a
  patient, a patient's legal guardian, or a person with the patient's durable
  power of attorney will be encrypted", with a Windows decryption tool: pick
  the CD up in person to get it unencrypted. Echocardiograms come from
  cardiology, not radiology.
- **Northwest.** "For imaging records (x-rays, scans, ultrasounds), except for
  dental or eye films, contact the Imaging ROI department" (phone on the
  page). Dental and eye images come from the Release of Information
  department "via email or a download portal": Nuance PowerShare, whose
  patient accounts offer "Download DICOM"; "Shared Images" are deleted after
  45 days. "Electronic records are provided free of charge to patients."
  The delivery format for radiology studies is not published; ask for a
  download link or USB rather than a CD.
- **Other regions.** Their records-request pages (read 2026-11-12) do not
  mention images; use a general HIPAA right-of-access request.

Every route ends in `medrec images intake` on the received folder, zip or disc
image. A download portal is used by the person in a browser; no tool calls an
image-exchange vendor's API.

## Claims through Epic

Epic serves health-plan claims as `ExplanationOfBenefit (Claim)`:
"ExplanationOfBenefit (Claim) resources represent claims data received and
processed by health plans, including services rendered to a patient and the
cost information associated with those services. … Subject to configuration by
the organization, this API can return resources built from pending claims, or
claims processed or paid by an external organization. Resource information for
these claims might be unverified, incomplete, or duplicative." It returns what
the plan adjudicates in Epic's payer software, Tapestry.

| Source (read 2026-11-16) | What it says |
|---|---|
| Epic's API catalog (`https://fhir.epic.com/Specifications/Selections`, the JSON behind the Specifications page) | `ExplanationOfBenefit.Read (Claim) (R4)` (id 1072) and `ExplanationOfBenefit.Search (Claim) (R4)` (id 1073): `"IsUSCDI": true`, API type "USCDI", for "Backend Systems and Non-OAuth 2.0, Clinicians or Administrative Users, Patients". `Coverage.Read` and `Coverage.Search (Patient Insurance Information) (R4)` likewise |
| Epic, Patient-Facing Apps Using FHIR | The USCDI v3 appendix names no `ExplanationOfBenefit`. A separate "CMS Payer Apps" auto-download path: the app "Uses only ExplanationOfBenefit (of any supported version) and/or QuestionnaireResponse.Read (Prior Auth) and/or any FHIR APIs … from the list in the appendix", selects "CMS Patient Access API", and the organization "Is marked to use CMS Payer APIs in their open.epic licensing agreement". Also: "Tapestry customers with Autodownloads turned on will automatically connect with patient access applications" |
| Kaiser's CapabilityStatements (Northwest, Washington, Northern California, Colorado) | `ExplanationOfBenefit`: `read` and `search-type`, by `patient`, `created`, `service-date`, `type`, `use` and 19 more. `Coverage` likewise |
| Kaiser, KP ClaimsConnect notice to providers | California: "The new claims platform we will be using is called Kaiser Permanente ClaimsConnect, based primarily on Epic's Tapestry® referral and claims software", cut over in three phases during 2016. Nothing found for Northwest or Washington |

So California's claims are in Epic, and every region's Epic declares the EOB
API. Neither shows that Kaiser's Epic returns claims to a patient's app: Kaiser
meets the CMS Patient Access rule on a separate server (Smile CDR, below), and
Epic's EOB returns only what the organization has configured.

### The test

Run once `mychart` has a production client id that Kaiser's Northwest
authorization server accepts (the authorize-URL check above). A member signs
in through `mychart` v0.5.1 or later at Kaiser Permanente - Northwest, as
the mychart profile for that kp.org account
(`mychart auth login --profile adult1-kpnw --org 132 --label adult1`), with mychart's
`redirect_uri` set to its https page: Epic takes only `https` redirects in
production, and mychart sends the page from v0.4.0. The read names the
organization too: the same label may be signed in at another organization,
and `--patient adult1` alone is then ambiguous (exit 4). One `mychart auth
login` runs at a time, since the https page forwards to one loopback port.
mychart's window opens kp.org's sign-on page and the lowercase Start page:
the member signs in with the code Kaiser sends, picks the longest sharing
period and approves ([Sign-in goes through kp.org](#sign-in-goes-through-kporg)).
Then one read, whose
output is statuses and counts only. The commands are
[`mychart`](https://github.com/piekstra/mychart-cli)'s (v0.5.0 or later:
every one takes `--profile`; `patients list` prints `patient-list/v1`,
`api GET` one FHIR read as the signed-in patient, and a refused read exits
3), not `kp`'s. The read
counts what Epic says it left out too: an `OperationOutcome` issue coded
`suppressed` (Epic code 59204, "Client not authorized for <type> - <API>")
in a 200 answer, which mychart's sandbox found Epic sends for an API the
app may not use:

```sh
p=$(mychart patients list --profile adult1-kpnw --json | jq -r '.items[] | select(.label=="adult1" and .organization=="132") | .id')
mychart api GET "ExplanationOfBenefit?patient=$p&_count=50" --profile adult1-kpnw --patient adult1 --org 132 --json \
  | jq -c '{type: .resourceType, total,
      entries: ([.entry[]?.resource.resourceType] | group_by(.) | map({(.[0]): length}) | add),
      withheld: [.entry[]?.resource | select(.resourceType == "OperationOutcome") | .issue[]? | select(.code == "suppressed")]
        | {all: length, claim: (map(select((.diagnostics // "") | test("claim"; "i"))) | length)}}'
```

| Answer | Means |
|---|---|
| exit 3 "access to it was refused" (a 403, or no `ExplanationOfBenefit` in the token's scope), or exit 4 | Northwest serves no EOB through Epic: register at developer.kp.org for `kp` |
| a `Bundle` with no `ExplanationOfBenefit` entries and `withheld.claim` 0, for a member who has had care outside Kaiser | Northwest's claims are not in Epic: register for `kp` |
| `withheld.claim` above 0, with or without `ExplanationOfBenefit` entries | Epic did not let mychart's app read claims there: it says nothing yet about whether Northwest's claims are in Epic. Check the app's API list for `ExplanationOfBenefit.Search (Claim)`, then repeat |
| a `Bundle` with `ExplanationOfBenefit` entries and `withheld.claim` 0 | Epic serves Northwest's claims, and `mychart records export` (v0.2.1 or later, which searches `ExplanationOfBenefit` at every organization) brings them to medrec. Whether they are all of them (Kaiser's API covers dates of service from 2016-01-01) shows only beside `kp`'s count for the same member |

Kaiser's claims are mostly for care outside Kaiser: "Kaiser Permanente in
California currently processes referred care and emergency services claims
from providers on multiple systems" (the ClaimsConnect notice). A member with
no outside care may have no EOBs at either server, which tells nothing.

Epic ended its Outside Record APIs (data other organizations sent) in its
November 2025 version, but its catalog has no Outside Record
`ExplanationOfBenefit` API, only `(Claim)` and `(Prior Auth)`, so that
change leaves the test as it is. A claim Kaiser processed for outside care is
Kaiser's own record, not an outside one.

### The result

Run 2026-11-19 at Kaiser Permanente - Northwest, for one member, with
`mychart` v0.5.1:

| Field | Value |
|---|---|
| Answer | a `Bundle`, `total` 0, no next page |
| `ExplanationOfBenefit` entries | 0 |
| `OperationOutcome` issues | 59204 `suppressed`: "Client not authorized for ExplanationOfBenefit - Prior Auth"; 4101 `processing`: "Resource request returns no results"; 4119 `processing`: the answer "may not contain the entire record" |
| `withheld.claim` | 0 |

That is the second row's answer: Epic let mychart's app search the Claim
sub-type and returned no claims. Only the Prior Auth sub-type was withheld,
and the test's `claim` filter rightly does not count it. A patient app
cannot register it: of the EOB APIs, Epic's registration form offers a
patient app only `.Read (Claim)` and `.Search (Claim)`, and lists
`ExplanationOfBenefit.Search (Prior Auth)` (id 11054) under Managed Care
(mychart's registration, 2026-11-16). So every Kaiser search through
`mychart` carries that 59204 notice. The row's conclusion, that Northwest's claims are not in
Epic, also needs a member who has had care outside Kaiser, and this record
does not say whether the member has: zero through Epic cannot tell
"Northwest's claims are not in Epic" from "this member has none". `kp`'s
count for the same member shows which. Until then Epic has given no claims,
so `kp`, through the Patient Access API Kaiser publishes for them, is the
claims route.

## Claims: Kaiser's Patient Access API

From Kaiser's "Patient Access API – Explanation of Benefits Implementation
Guide" (August 2023, 23 pages) and the live CapabilityStatement.

| Item | Value |
|---|---|
| FHIR base | `https://kpx-service-bus.kp.org/service/cdo/siae/healthplankpxv1rc/FHIR/api` (the guide's search table also writes `[base]/HPEOB/ExplanationOfBenefit`; its Postman example omits `HPEOB`) |
| Authorize | `https://kpx-service-bus.kp.org/service/kpx/v1/oauth2/authorize`, which redirects to PingFederate at `https://pfam.kp.org/as/authorization.oauth2`; that answers an unregistered client id with 400 "Invalid client_id" (2026-11-17) |
| Token | `https://kpx-service-bus.kp.org/service/kpx/v1/oauth2/token` |
| Discovery | No `.well-known/smart-configuration` (Apigee answers 400 "Unauthorized", `AMP-Security-134`); the OAuth URLs are only in the CapabilityStatement's `oauth-uris` extension |
| Client | Confidential, "Symmetric authentication approach": client id and secret, sent as an HTTP Basic header |
| PKCE | Required, S256 |
| Scopes, production | `patient/*.read launch/patient offline_access ExplanationOfBenefit` |
| Scopes, sandbox | the same plus `sandbox` |
| `aud` | "Optional but must be a valid URL" |
| `state` | Optional |
| Resources | `ExplanationOfBenefit` (search by `patient`, `_lastUpdated`, `created`; `_include` of `patient`, `provider`, `*`), and reads of `Coverage`, `Location`, `Organization`, `Patient`, `Practitioner` by the ids an EOB references. The CapabilityStatement also lists `Encounter` and `MedicationDispense` |
| Server | Smile CDR (`implementation.description` "FHIR Endpoint powered by Smile CDR"), not Epic. Its CapabilityStatement (read 2026-11-16) lists `patient` and `_lastUpdated` as search parameters of `ExplanationOfBenefit`, `Coverage`, `Encounter` and `MedicationDispense`, so Epic's limit of some types to `_id` searches does not apply; `Location`, `Organization`, `Patient` and `Practitioner` search only by `_id`, `identifier` and the like, and kp reads them by reference |
| Data | Dates of service from 2016-01-01; claims are loaded as received or finalized |
| Regions | All: Northern and Southern California, Colorado, Georgia, Hawaii, Mid-Atlantic States, Northwest, Washington |
| Revocation | The member removes the app in kp.org: Profile and Preferences, Data sharing & authorization, Manage access to health plan information |

Sandbox: the guide publishes 10 synthetic test members (`Pvaluser1` to
`Pvaluser10`) signing in at a "test kp.org prod account" page, among them a
family where one member is proxy for a spouse and two children
(`Pvaluser3`), with EOBs for sensitive procedures and diagnoses hidden from
proxies and from members under 18.

### What kp does with it

| Step | Request | On failure |
|---|---|---|
| Discovery | `GET [base]/metadata`, endpoints from `rest[0].security.extension` `oauth-uris` (`KaiserQuirks::metadata_first`; no `.well-known` request) | exit 5 |
| Sign-in | authorize with PKCE S256, `aud` = the base, the scopes above, in Chrome in the profile's own browser profile with no page before it (its authorize step goes to `pfam.kp.org`, not kp.org's member sign-in); token request with the client id and secret as HTTP Basic | exit 3; exit 2 (reason `no-client`) when no client is stored for the environment |
| Member | `GET Patient/<patient>` from the token response's `patient` | skip `Patient` |
| Searches | `ExplanationOfBenefit`, `Coverage`, `Encounter`, `MedicationDispense`, each `?patient=<id>`, every page | skip the search: `forbidden` (403), `unsupported` (404), `upstream`, `page_cap`; an answer whose `OperationOutcome` has an issue coded `suppressed` keeps its results, and the trailer has one `forbidden` skip per type and distinct line (`SearchResult::withheld`), with `resource_type` and no `search` or `id` |
| `--since D` | `&_lastUpdated=geD` on each search; the member's `Patient` and every referenced read come whole (`since_unbounded`) | as above |
| References | a read of each `Coverage`, `Location`, `Organization`, `Patient`, `Practitioner` the results name, relative or absolute on the base (scheme and host in any case), not yet written; contained (`#…`), `urn:`, conditional and other servers' references are left alone | skip that resource (`forbidden`, `not_found`, `upstream`); a reference on the base whose id FHIR does not allow is an `invalid_reference` skip naming the type and the id's length, never the id |
| Renewal | before a request, a lapsed access token is renewed with the refresh token; a 401 is renewed once and the request retried | stop: exit 3, `kp auth login`; exit 2 (`no-client`) when no client is stored for the environment |
| Stored sign-in | the member's pk-med-smart session, the vault entry `<profile>/<org>/<member>` | stop: exit 1, `kp auth logout` then `kp auth login` (each with `--profile`) when it does not read, `kp self-update` when a newer kp wrote it |

The export stops, with `complete: false` in the trailer and the error's exit
code, only when the sign-in is refused, the server is unreachable or the
output fails. Kaiser's guide does not say how long its access and refresh
tokens last or whether a refresh rotates the refresh token; pk-med-smart
stores whatever the token response returns. In the sandbox the access token
lasted one hour and the consent one year.

Kaiser's guide (V4, "Supported Resources") supports one search, of
`ExplanationOfBenefit` by `patient`, `_lastUpdated`, `created` and
`_include`, and reads of the `Coverage`, `Location`, `Organization`,
`Patient` and `Practitioner` it references. The CapabilityStatement declares
`Encounter` and `MedicationDispense` too, but the sandbox answers both
searches with 404. kp still searches them: a 404 is an `unsupported` skip,
and production may serve what the server declares.

`kaiser-health-plan` (production) and `kaiser-health-plan-sandbox` are
separate organizations in `orgs list`, with separate client credentials
(the vault entries `_client/production`, `_client/sandbox`) and sign-ins
(`<profile>/<org>/<patient>`), so a sandbox member never lands in the household's
records as a real one. A production sign-in goes to Kaiser's server only
(https on `kp.org`); a sandbox one to Kaiser's or to a server on this
machine (medical-common's mock). kp refuses any other `fhir_base` before
the client id and secret are read. `-o <DIR>` writes only the sandbox's records to
a file.

### Kaiser's sandbox

Run 2026-11-19 with kp v0.2.2, the app's sandbox client and the redirect
`http://127.0.0.1:7469/callback`, all synthetic members:

| Step | Result |
|---|---|
| Sign-in | `pfam.kp.org` "Sign in" (`pf.username`, `pf.pass`), then `kpx-consent.kp.org` "Consent Management": one choice per member the account may act for, the data ("ExplanationOfBenefit"), access "until" one year later, an "I have read the statements above" box, then ACCEPT or DENY. Stored and verified |
| `Pvaluser1`'s export | 7 resources: 1 `ExplanationOfBenefit`, 1 `Coverage`, 1 `Patient`, 1 `Practitioner`, 3 `Organization`; `Encounter` and `MedicationDispense` skipped `unsupported`; complete |
| `--since 2026-01-01` | no resources (the sandbox's data is older), the same two skips, complete |
| A proxy: `Pvaluser3` signs in and picks `Pvaluser6`, under 18 | the consent page lists four members, the account's own and the three it acts for. Stored under the label given; the export has 1 `Coverage`, 1 `Patient`, 1 `Organization` and no claims, as the guide hides sensitive claims from proxies and from members under 18 |
| `relationship` | unknown for both: kp asks for Kaiser's scopes, which have no `openid` or `fhirUser`, so the token names the member and not who signed in |
| A sign-in nobody finishes | exit 3 at `--timeout`, nothing stored; from v0.2.3 the error names the `kp auth login` command that starts it again |

### Registration at developer.kp.org

[registration.md](registration.md) walks through the portal and
summarizes its terms. What the portal showed (2026-11-19) beyond the
sources below:

- The sign-up (Azure AD B2C) asks for a "valid company email address" and
  took a personal one; its password must be 8 to 16 characters.
- The sandbox's redirect URI and "Generate ID/Secret" work as soon as the
  API is requested, before any approval, and the `http` loopback redirect
  `http://127.0.0.1:7469/callback` was accepted there.
- The production redirect URI stays read-only until Kaiser approves the
  request by email. Kaiser publishes no time frame for that review and no
  rules for loopback redirects in production. Should it take only `https`,
  as Epic does in production, pk-med-smart v0.5.0 can send a registered
  `https` page that forwards to the loopback (`SmartApp::redirect_uri`);
  kp would need that page hosted and a setting naming it.
- Each client secret is shown once.
- Nothing on the portal, in the guide or in the terms mentions a fee.
  Kaiser "recommend[s] that your application includes a clear privacy
  policy" (CMS-9115-F).

## Sources (read 2026-11-12 unless dated)

- Kaiser developer portal: `https://developer.kp.org/#/home`, the API catalog
  `https://developer.kp.org/#/apis` and its data
  `https://developer.kp.org/api/ext/services`. The FAQ needs a signed-in account.
- KP Patient Access API EOB Implementation Guide V4:
  `https://developer.kp.org/api/ext/services/639c015049655aa96ab5b2f1/documents/64dd48f306f151907ebc2405`
- KP Clinical Data API, Developer Guide for Interoperability (June 2023):
  `https://developer.kp.org/api/ext/services/63e333b9b73c1ef28353d389/documents/649c8ca883ae11a3177d0499`
- KP Developer Portal Terms and Conditions:
  `https://developer.kp.org/assets/data/terms-and-conditions.pdf`
- Kaiser technical information, API Developer Portal section:
  `https://healthy.kaiserpermanente.org/northern-california/pages/technical-information#api-developer-portal`
- Kaiser Third-Party App Privacy:
  `https://healthy.kaiserpermanente.org/support/health-record-access-update/third-party-app-privacy`
- kp.org Terms & Conditions (read 2026-11-17):
  `https://healthy.kaiserpermanente.org/termsconditions`
- kp.org multifactor authentication (read 2026-11-17):
  `https://healthy.kaiserpermanente.org/pages/online-security/multifactor-authentication`
- kp.org sign-in (read 2026-11-17): `https://healthy.kaiserpermanente.org/sign-on`
  and the scripts it loads
- Kaiser Act for a Family Member:
  `https://healthy.kaiserpermanente.org/northern-california/support/act-for-a-family-member`
- Kaiser records and imaging requests, per region:
  `https://healthy.kaiserpermanente.org/<region>/support/medical-requests`
  (Washington's radiology section; Northwest's "Order an electronic copy of my
  detailed medical records"), the Washington release form
  `https://healthy.kaiserpermanente.org/content/dam/kporg/final/documents/forms/release-authorization-wa-en.pdf`
  and Northwest's PowerShare guides `.../forms/patient-guide-nw-en.pdf` and
  `.../forms/powershare-mobile-app-nw-en.pdf`
- syntropy-health PR 22, "Kaiser: work around the broken sign-in page, and
  follow its paging links" (merged 2026-10-03, read 2026-11-19), for the
  lowercase Start path:
  `https://github.com/EnriqueNeyra/syntropy-health/pull/22`
- Epic API specifications: `https://fhir.epic.com/Specifications`
  (`Media.Read (Study) (R4)`, `Binary.Read (Study) (R4)`)
- Epic endpoint directory: `https://open.epic.com/MyApps/Endpoints`,
  `https://open.epic.com/Endpoints/Brands`, `https://open.epic.com/Endpoints/R4`
- Epic, Patient-Facing Apps Using FHIR (automatic client id distribution):
  `https://fhir.epic.com/Documentation?docId=patientfacingfhirapps`
- Epic's API catalog as JSON (read 2026-11-16):
  `https://fhir.epic.com/Specifications/Selections`, and one API's
  description at `https://fhir.epic.com/Specifications/Api?id=<id>` (1073:
  `ExplanationOfBenefit.Search (Claim) (R4)`)
- Kaiser, KP ClaimsConnect (read 2026-11-16):
  `https://healthy.kaiserpermanente.org/content/dam/kporg/final/documents/community-providers/national/ever/kp-claims-connect-overview-en.pdf`
  and `.../kp-claims-connect-platform-en.pdf`
- Each Kaiser base's `.well-known/smart-configuration` and `metadata`, and the
  claims server's `metadata`.
