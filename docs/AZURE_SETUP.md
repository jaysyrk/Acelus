# Azure app registration and Mojang approval

Acelus authenticates against real Microsoft accounts and verifies real game ownership. That
requires an Azure application that Mojang has explicitly approved. Until approval lands,
`api.minecraftservices.com` answers every request with **403**, no matter how correct your code is.

Approval takes days, and up to a further 24 hours to propagate. **Start this before writing any
auth code.**

## 1. Register the application

In the [Azure portal](https://portal.azure.com) under *App registrations* > *New registration*:

| Setting | Value | Why |
|---|---|---|
| Supported account types | **Personal Microsoft accounts only** (`consumers` tenant) | Minecraft accounts are consumer accounts. Choosing a work/school or multi-tenant option makes `XboxLive.signin` fail in ways the error messages do not explain. |
| Redirect URI | *Public client/native* > `http://localhost` | Loopback redirect for the authorization code flow. No HTTPS certificate needed on loopback. |
| Client secret | **Do not create one** | Acelus is a public client. It ships to users' machines, so a secret in the binary is not a secret. |

Then under *Authentication*:

- Set **Allow public client flows** to **Yes**. This enables the device code flow, which is what
  Acelus uses by default. Without it, device code login fails with `unauthorized_client`.

Record the **Application (client) ID** from the *Overview* page. There is no tenant ID to record —
Acelus always uses the `consumers` tenant.

## 2. Generate activity, then request approval

The ordering here is the part that trips people up, and it is deliberate:

1. **Attempt a login first.** Run the auth chain end to end with your new client ID. It will get as
   far as a valid XSTS token and then fail at `login_with_xbox` with **403**. That is the expected,
   correct outcome for an unapproved app.
2. **Then submit <https://aka.ms/mce-reviewappid>.** Microsoft wants to see that the application has
   actually been used before they will review it. Submitting without a login attempt on record
   tends to go nowhere.

Applications registered before this policy came into effect are grandfathered and keep working.
New ones are not.

## 3. Configure Acelus

The client ID is configuration, never a compile-time constant. Acelus reads it in this order:

1. `ACELUS_CLIENT_ID` environment variable
2. `client_id` in `$XDG_CONFIG_HOME/acelus/config.toml`
3. The built-in default used by official Acelus releases

This means the auth chain can be developed and tested in full before approval arrives, and anyone
forking Acelus can drop in their own approved application without patching source.

## 4. Confirming approval landed

Re-run the login. `login_with_xbox` returning a Minecraft access token instead of 403 means the
application is approved. If it still 403s more than 24 hours after Microsoft confirmed approval,
the usual cause is a mismatched client ID rather than a stale approval.

## What approval does not mean

Approval grants API access. It does not make Acelus an official Minecraft product, and does not
imply Mojang or Microsoft endorse it. The README states this plainly and so should any distribution
of the project.
