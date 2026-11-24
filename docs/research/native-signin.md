# Native KONAMI ID sign-in

Investigated 2026-11-22. Anonymous requests and one made-up address (`konaste-hub-probe-nonexistent@example.invalid`) only. No real account, password or passkey was used.

## Flow

`GET https://p.eagate.573.jp/gate/p/login.html` redirects to `https://my.konami.net/api/authorizations` (client `KRXaB58UFTtra9ZWUERRHMziPUXrFhwH`, callback `/gate/k/login_exec.html`), then to `/{locale}/signin`. The sign-in page is a Next.js app. Its bundles (`my-assets.service.konami.net/_next/static/chunks/*`) call these same-origin JSON endpoints:

| Step | Request | Answer |
|---|---|---|
| Auth type | `POST /api/auths/login/authTypes` `{"id"}` | 201 `{"authType":"PASSWORD"\|"2STEP"\|"PASSKEY"\|"CODE"}` (made-up IDs also get `PASSWORD`) |
| Captcha | `GET /api/captchas` | 200 `{"correctPictureUri","testPictureUris":[5]}`; images at `/api/captchas/picture?token=` (100x100 PNG) |
| Password | `POST /api/logins` `{"id","password","captchaAnswers":"01001"}` | 200 `{"redirectUri"}`; 400 `{"code"}`: 400005003 captcha, 400008003 credentials |
| Email code | `POST /api/auths/login/codes` `{"id"}`, then `POST /api/logins` `{"id","captchaAnswers":"","code"}` | 201 `{"isIssued":true}`; 400061001 wrong code |
| Two-step | page `/signin/two-step-verification`; `GET /api/auths/twoStepVerification?code=&isSkipped=` | 200 `{"redirectUri"}`; 400014003 wrong code |
| SMS | page `/signin/sms-verifications`; `GET /api/auths/smsCodeVerification/codes/{code}`, resend `POST …/codes` | 200 `{"redirectUri"}` |
| Passkey | `POST /api/tokens/passkeyAuthentication` `{"loginId"}` | 201 `{"passkeyAuthenticationRequest":"{\"publicKey\":{challenge,timeout:300000,rpId:\"my.konami.net\",userVerification:\"preferred\"}}"}` |
| | `POST /api/auths/passkey` `{"authenticationResponse":"<SimpleWebAuthn JSON>"}` | 200 `{"redirectUri"}`; 400 rejected |

The page sends the PASSWORD and 2STEP types to the password form, and PASSKEY and CODE to the passkey or email-code form.

## Bot check

`my.konami.net` is behind Akamai Bot Manager (`_abck`, `bm_sz`, `ak_bmsc`, sensor script `/akam/13/…`). Read-only endpoints (`authTypes`, `captchas`, `passkeyAuthentication`, `auths/login/codes`) answered plain HTTP clients. `POST /api/logins` did not:

| Client | Result for the made-up login |
|---|---|
| Node `fetch` with the page's cookies | 403 Akamai HTML page |
| Playwright-driven Chrome (`navigator.webdriver` true) | 403 |
| Chrome started normally, request run in the page | 400 `400005003` (reached KONAMI) |
| Hidden WebView2 (wry), request run in the page | 400 `400005003` (reached KONAMI) |

A native HTTP client therefore cannot send the password itself. The Hub keeps the official page loaded in a WebView2 that is never shown and sends each request from inside it (`crates/konaste-hub/src/browser.rs`). The form, captcha and errors are drawn natively (`signin.rs`, `ui/view.rs`).

## Passkeys

The WebAuthn options are standard and name `rpId` `my.konami.net`. The Hub calls `webauthn.dll` (`WebAuthNAuthenticatorGetAssertion`) with client data for origin `https://my.konami.net`, then posts the result in SimpleWebAuthn's `AuthenticationResponseJSON` shape (`crates/konaste-hub/src/passkey.rs`). In a test with no passkey, Windows Security opened "Choose a passkey: my.konami.net". No assertion was sent to KONAMI; whether the server accepts a desktop-client assertion is **not yet observed**.

## Not covered natively

Consent, birthday, country, account lock and other `/signin/*` pages after a successful step. The Hub then shows a link to My KONAMI and asks the user to sign in again afterwards.
