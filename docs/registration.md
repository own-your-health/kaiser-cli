# Registering an app for kp at Kaiser

`kp` ships without a client id. Kaiser's Patient Access API answers only a
registered app. Kaiser issues each app a client id and secret, one pair for
its sandbox and one for production, and its developer terms keep both
confidential. So each person, or each household, registers an app of its
own at [developer.kp.org](https://developer.kp.org) and stores the pairs in
kp.

`kp registration` prints every value below for production's client, and
`kp registration --sandbox` for the sandbox's, on the configured redirect
port (`--json` gives medical-common's `registration/v2`). The portal's labels are quoted as the
portal showed them in November 2026; Kaiser's own wording, with sources, is
in [api.md](api.md).

| | Sandbox | Production |
|---|---|---|
| Members | Kaiser's synthetic test members | real kp.org members |
| Approval | none: works as soon as the API is requested | Kaiser reviews the request and answers by email; no time frame is published |
| Stored with | `kp auth set-credential --sandbox` | `kp auth set-credential` |

Nothing on the portal, in Kaiser's guide or in its terms mentions a fee.

## Before you start

- An email address. The sign-up asks for "valid company email address"; a
  personal address was accepted.
- A password of 8 to 16 characters (longer is refused) with three of the
  four: lowercase letters, uppercase letters, digits, symbols.
- A phone that takes a text or a call, for the portal's sign-in code.
- Later, to sign in through kp: the member's kp.org account.

Steps 1, 5 and 6 need you at the screen: the email and phone codes, and
copying the secret. kp cannot do them.

## 1. Create a developer account

1. Open <https://developer.kp.org> and choose **Create an Account**. A
   dialog asks you to tick "I agree to the Terms and Conditions" and
   choose **Accept**. [The terms](#the-terms) has their main points.
2. The sign-up page ("User Details", on `apieprd.b2clogin.com`, Kaiser's
   Azure AD B2C) says "Please use valid company email address. Kaiser
   Permanente Employees should use internal portal." Enter **Email
   Address** and choose **Continue**.
3. On the next page choose **Send verification code**, copy the code from
   the email into **Verification code**, and choose **Verify code** ("E-mail
   address verified. You can now continue."). Then fill in the fields, all
   required:

   | Field | What to enter |
   |---|---|
   | Password, Confirm Password | 8 to 16 characters, with three of: lowercase letters, uppercase letters, digits (0-9), and one or more of ``@ # $ % ^ & * - _ + = [ ] { } \| \ : ' , ? / ` ~ " ( ) ; .`` A 24-character password is refused. |
   | Display Name, First Name, Last Name | Your own. |
   | Company/Business Name | A company is assumed. Someone registering for their own records can say so: `Individual`. |
   | Country/Region | Yours. |

   Choose **Continue**.
4. The phone page: "Enter a number below that we can send a code via SMS or
   phone to authenticate you." Pick the **Country Code**, enter the **Phone
   number**, choose **Send Code** (a text) or **Call Me**, and enter the
   code.

## 2. Choose the Developer persona

The console opens on **My Applications** ("Let's get started. Tell us a bit
about who you are."). Choose **Persona Setup**, click the **Developer** card
itself, and choose **Submit Persona Setup** ("Selected Persona(s) submitted
successfully."). A click on the card's title alone leaves it unselected, and
the submit then fails with "Error The server cannot process this request."

The other persona, **Prior Auth**, gives access to Kaiser's Prior
Authorization API, which kp does not use.

## 3. Add the application

Choose **Add Application**. The dialog "Create New Application":

| Field | What to enter |
|---|---|
| Application Name (Public) | A name the members you sign in will recognise, for example `Family Health Records (kp)`. Kaiser shows it to members on kp.org's list of apps with access and asks for no names like "test" or "validation". It cannot be edited after saving. |
| Application Website | `https://github.com/own-your-health/kaiser-cli`, or your own copy. |
| Application Description | `A command-line tool that a Kaiser Permanente member runs on their own computer to download their own health-plan claims and coverage, and those of the family members they act for on kp.org, into their own files. It sends the records to no one else.` Members see it too. It cannot be edited after saving. |
| Persona or Type | **Developer** |

The dialog has no logo, privacy policy or terms fields. Choose **Save**. The
application's row on My Applications has a **Details** button, which opens
the page used in steps 5 and 6.

Kaiser "recommend[s] that your application includes a clear privacy
policy". kp's is [PRIVACY.md](../PRIVACY.md), linked from the README the
website opens.

## 4. Request the API, and get Kaiser's guide

1. Open **API Catalog**. Under **Secure**, open "Patient Access API -
   Explanation of Benefits and Associated Resources" (Version 4.0.1 FHIR;
   the page shows "Visibility: partner" and "Applicable Persona:
   Developer").
2. Choose **Use this API**, select the application, and choose **Request
   Access**. The application's Details page lists the API under "APIs
   Consumed by this Application" with **Production Access Status**
   "Pending". Kaiser reviews the request and emails the decision; it
   publishes no time frame.
3. On the same catalog page, signed in, choose the link
   "Patient_Access_API_Secured_Implementation Guide_V4". A popup downloads
   Kaiser's implementation guide (PDF). Its address needs the portal's
   sign-in, so a plain download of it fails. The guide's "Test Accounts"
   table has the sandbox's test members and their passwords.

Leave "KP Interoperability Clinical Data" alone: it sends you to Epic's
portal. Kaiser's clinical records come through
[`mychart`](https://github.com/own-your-health/mychart-cli) ([api.md](api.md),
Route).

## 5. The sandbox client, right away

On the application's Details page ("You currently have access to the
Sandbox environment"):

1. **Sandbox Redirect URI**: enter `http://127.0.0.1:7469/callback` and
   choose **Add sandbox uri** (it is "Needed before you can generate your
   ID/Secret"). The portal takes the `http` loopback address for the
   sandbox. If you change kp's port (`kp config set redirect_port`), enter
   that port: Kaiser compares the address with the one kp sends.
2. Choose **Generate ID/Secret**. The **Sandbox Client ID** appears on the
   page, and the dialog "Client Secret" shows the secret once: "You will
   not be able to see this information again." Copy it into your password
   manager before choosing **OK**.
3. Store the pair in kp:

   ```sh
   kp auth set-credential --sandbox     # asks for the id, then the secret, without echoing
   op read "op://<vault>/<item>/pair" | kp auth set-credential --sandbox --stdin   # or `<id>:<secret>` from 1Password
   ```

4. Try a test member from the guide's table:

   ```sh
   kp profiles add sandbox
   kp auth login --profile sandbox --org sandbox --label pvaluser1
   kp records export --profile sandbox --patient pvaluser1 --ndjson | head -3
   ```

   Chrome opens Kaiser's test sign-in ("Sign in", on `pfam.kp.org`), then
   its "Consent Management" page: pick the member, tick "I have read the
   statements above" and choose **ACCEPT**. The window closes when kp has
   the sign-in.

## 6. The production client, after Kaiser approves

Once Kaiser's email approves the request, the **Production Redirect URI**
field on the Details page, read-only until then, takes input:

1. Enter the same `http://127.0.0.1:7469/callback`, add it, and choose
   **Generate ID/Secret**. Copy the secret: it is shown once.
2. `kp auth set-credential` (or `--stdin`, as above).
3. Each kp.org account gets a profile, and each member a sign-in, as the
   [README](../README.md#quick-start) shows. On a real account the member
   signs in and approves sharing on Kaiser's pages; kp never fills them.

Kaiser publishes no rules for loopback redirects in production. If it takes
only `https` there, as Epic does, kp needs an `https` page that forwards to
the loopback address, and a setting naming it, which it does not have yet.

## What kp uses

Nothing here goes in a form; Kaiser fixes it, and kp sends it.

| Item | Value |
|---|---|
| Launch | Standalone patient launch: the member signs in from kp, not from a portal |
| Client | Confidential, Kaiser's "Symmetric authentication approach": the client id and secret as an HTTP Basic header at the token endpoint |
| PKCE | Required, S256 |
| Scopes, production | `patient/*.read launch/patient offline_access ExplanationOfBenefit` |
| Scopes, sandbox | the same, plus `sandbox` |
| Redirect | `http://127.0.0.1:<redirect_port>/callback`, port 7469 unless configured |
| Endpoints | read from the FHIR server's CapabilityStatement ([api.md](api.md)) |

In the sandbox an access token lasts an hour and the member's consent a
year; kp renews the token with the refresh token on its own. Kaiser's guide
does not give production's lifetimes.

## Check what is stored

```sh
kp info                            # `clients`: a row per environment, configured or not, without the Keychain
kp auth status --profile <NAME>    # the same rows read from the vault, with the profile's sign-ins
```

`info` sees that a client's entry is in the vault; `auth status` reads it.
So an entry that no longer decrypts, after the vault's Keychain key was lost
or replaced, is configured in `info` and has an `error` in `auth status`'s row.

A command that needs a client that is not stored fails with exit 2
(reason `no_client`), naming the environment, this guide, that
environment's `kp registration` command and its `kp auth set-credential`
command.

## Ending access

- A member removes the app on kp.org: Profile and Preferences, Data sharing
  & authorization, Manage access to health plan information.
- On the portal, the Details page has **Unsubscribe from API** and **Delete
  Application**.
- `kp auth logout --profile <NAME> --forget` deletes the profile's sign-ins
  and, once no other profile holds one, the stored client ids and secrets.

## The terms

The KP Developer Portal Terms and Conditions (dated 20/4/2026):

- They are a contract with Kaiser Foundation Hospitals under California law,
  with exclusive jurisdiction in Alameda County courts.
- They assume a company: the person agreeing is "authorized to enter into
  this Agreement on behalf of your Company", and "any and all registration
  information you provide" must be accurate.
- A limited, non-exclusive, royalty-free licence to use the KP APIs "as
  described in their applicable implementation guides".
- No reverse engineering of the APIs, no exceeding rate limits, and no
  "security research on or testing against KP APIs" without written
  approval. Security flaws found in normal use go to the Kaiser National
  Compliance Hotline and are not disclosed publicly.
- Credentials (passwords, keys, tokens, user ids) stay confidential and go
  to no third party: keep the client id and secret in kp's vault and your
  password manager, out of files, chats and issues.
- Kaiser may monitor use, suspend or revoke credentials without notice, and
  terminate "for any or no reason".
- The developer indemnifies Kaiser for claims arising from use of the APIs
  or of information obtained through them; Kaiser disclaims all warranties
  and liability.

Source: `https://developer.kp.org/assets/data/terms-and-conditions.pdf`.
