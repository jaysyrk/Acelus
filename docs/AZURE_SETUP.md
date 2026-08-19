# Azure app registration and Mojang approval

Acelus authenticates against real Microsoft accounts and verifies real game ownership. That
requires an Azure application that Mojang has explicitly approved. Until approval lands,
`api.minecraftservices.com` answers every request with **403**, no matter how correct your code is.

Approval takes days, and up to a further 24 hours to propagate. **Start this before writing any
auth code.**

## 0. You need a directory before you can register anything

App registrations live in a directory, and a personal Microsoft account does not come with one.
Signing in to the portal with one puts you in a restricted system tenant named **Microsoft
Services**, which has no directory attached. Attempting to register there fails with:

> Selected user account does not exist in tenant 'Microsoft Services' and cannot access the
> application 'c44b4083-3bb0-49c1-b47d-974e53cbdf3c' in that tenant.

That application id is the Azure portal itself, not anything you created. The message means the
sign-in never reached a usable directory.

Two ways to get one:

- **A work or school account**, if you have one. It already belongs to a real directory, so there
  is nothing to create. Administrators can switch off app registration for ordinary users, so this
  either works immediately or refuses immediately.
- **An [Azure free account](https://azure.microsoft.com/free)**, which creates a directory and
  makes you its Global Administrator. App registration itself costs nothing on the Entra ID Free
  tier, but signing up **requires a credit card for identity verification**. Microsoft states the
  card is not charged for the free tier.

Creating a bare tenant from the Entra admin center instead is not an option here: Microsoft
restricts *Manage tenants* > *Create* to paid customers.

## 1. Register the application

1. Sign in to <https://portal.azure.com> with an account that has a directory, per step 0.
2. Search **App registrations** in the bar at the top and open it.
3. Click **+ New registration**.
4. **Name**: anything. `Acelus` will do.
5. **Supported account types**: the last of the four options, **Personal Microsoft accounts
   only**. The option above it also mentions personal accounts but pairs them with organizational
   directories; that is not this one. Minecraft accounts live in the `consumers` tenant, and any
   other choice makes `XboxLive.signin` fail in ways the error messages do not explain.
6. **Redirect URI**: change the platform dropdown from *Web* to **Public client/native (mobile &
   desktop)** and enter `http://localhost`. Loopback needs no certificate.
7. Click **Register**.
8. On the **Overview** page that follows, copy the **Application (client) ID**. It sits above
   *Object ID* and *Directory (tenant) ID*, which are different values and not the one you want.
   There is no tenant ID to record: Acelus always uses `consumers`.
9. In the left sidebar under *Manage*, open **Authentication**, scroll to **Advanced settings**,
   and set **Allow public client flows** to **Yes**. Click **Save**. This enables the device code
   flow Acelus logs in with; without it login fails with `unauthorized_client`.

**Do not create a client secret.** Acelus is a public client that runs on the user's machine, so a
secret shipped inside it is not secret. The device code flow does not use one.

## 2. Generate activity, then request approval

The ordering here is the part that trips people up, and it is deliberate:

1. **Attempt a login first.** Run `acelus login` with your new client ID configured. Sign in with
   the code it prints. It will get as far as a valid XSTS token and then fail at `login_with_xbox`
   with **403**. That is the expected, correct outcome for an unapproved app, and the activity
   Microsoft wants to see.
2. **Then submit <https://aka.ms/mce-reviewappid>.** Microsoft wants to see that the application has
   actually been used before they will review it. Submitting without a login attempt on record
   tends to go nowhere.

Applications registered before this policy came into effect are grandfathered and keep working.
New ones are not.

## 3. Configure Acelus

The client ID is configuration, never a compile-time constant. Acelus reads it in this order:

1. `ACELUS_CLIENT_ID` in the daemon's environment
2. `client_id` in `$XDG_CONFIG_HOME/acelus/config.toml`, which on Linux means
   `~/.config/acelus/config.toml`

There is no built-in default. Acelus ships without a client ID, because an approved application
belongs to whoever registered it, and baking one in would hand every user's logins to that
registration.

```toml
client_id = "00000000-1111-2222-3333-444444444444"
```

Prefer the file. The daemon is the process that reads the client ID, and it outlives the shell
that started it, so a `ACELUS_CLIENT_ID` exported in one terminal is invisible to a daemon already
running from another. A malformed config is logged and ignored rather than being fatal.

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
