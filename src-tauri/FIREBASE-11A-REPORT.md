# Step 11a implementation report — incomplete

This is a tested initial Firebase layer, **not completion of the requested exact-parity port**. Passing counts describe the checked corpus, not all SDK behaviour. Do not start 11b on the assumption that all Firebase semantics are covered.

## API provided

`firebase::Auth` owns one in-memory auth instance. Public methods: `sign_in_with_email_and_password`, `create_user_with_email_and_password`, `send_password_reset_email`, `sign_out`, `update_profile`, `get_id_token`, and `fields`. `current_user` exposes uid, email, display_name and photo_url; tokens remain private. `on_auth_state_changed` registers event capture (including the initial value); `take_events` drains snapshots after operations. This is an event queue, not yet a general asynchronous subscription/unsubscribe API. The 11b adapter would own this singleton and dispatch these events.

`firebase::firestore::{doc,get_doc,set_doc,update_doc}` uses `Document`, `Snapshot::{exists,data}`, and IPC `JsValue`. `set_doc` means merge=true. Writes serialize through `write_body`; reads use `decode_fields`/`decode_value`. Network operations take `Host` and mutable `Auth`, obtaining/refreshing the signed-in token. `Error` exposes code and message.

`FetchRequest` now carries method and optional body. RealHost uses reqwest's request builder. Existing lyrics calls explicitly use GET/no body. MockHost retains legacy fetch-call recording and additionally records complete requests for Auth comparisons. No dependencies added. The existing base64url encoder is reused through crate visibility.

## Oracle and comparisons

Both new runners execute installed Firebase SDK 12.11.0 under Electron's Node. Global fetch is mocked before SDK import; no live Firebase requests are possible. Auth compares method, URL, normalized Headers entries, exact body string, result/error, and callback snapshots. Time is fixed on both sides. Scenarios use separate app names: the SDK's shared inMemoryPersistence survives deleteApp, so reusing the default name leaked state between otherwise independent scenarios. This isolation does not alter the SDK persistence implementation.

Firestore uses the Lite SDK's real parser/serializer. Writes are exercised through Host and their parsed REST bodies and errors compared. Reads compare equivalent returned documents and independently assert the Rust GET request. Lite sends POST documents:batchGet with a documents array; Rust intentionally uses the requested GET transport. These are different requests, not wire parity. The runner observes decoded object shapes through Node structuredClone, not an actual Electron IPC round trip.

## Differences found and corrected

- Auth's one-time heartbeat header includes registered Auth **and full Firestore** component versions and UTC date. It is included in comparisons, not removed.
- A near-expiry sign-in token refreshes **before** accounts:lookup. Refresh uses form encoding, with spaces as %20.
- Expiration uses a strict greater-than comparison against expiry minus 30 seconds.
- Server code mapping, custom ` : ` details, special tagged errors, non-JSON responses and network failures produce the tested exact messages/codes.
- MISSING_RECAPTCHA_TOKEN starts configuration GETs; the tested failed-config path performs two attempts.
- Profile/refresh USER_DISABLED and TOKEN_EXPIRED failures sign out and emit null; invalid credentials do not.
- Firestore distinguishes safe integers, doubles, negative zero and special floating values. Integer-looking doubles must retain the SDK's JSON number rendering.
- Merge masks use literal set keys, nested update paths, escaped field names, empty-map paths and sorted paths. Update preconditions require exists=true.
- Empty updateDoc({}) omits the update's fields member; empty setDoc maps retain it.
- Root validation precedes recursive parsing. A string update argument selects the SDK's field/value overload, yielding an undefined-field error.
- Arrays suppress field paths in errors, prohibit direct nested arrays, and permit arrays inside map elements.
- Raw Uint8Array is rejected; Date serializes with nine fractional timestamp digits.
- Read decoding tested for integer precision loss, special doubles, timestamps, bytes, geopoints, arrays/maps and vectors.

## Remaining implementation and verification gaps

- DocumentReference decoding is explicitly unimplemented. A probe of Lite referenceValue followed by structuredClone fails with `container => new PlatformLoggerServiceImpl(container) could not be cloned.` An actual full-SDK Electron IPC observation and suitable representation through the cloud-auth layer are still required. Returning a string would not preserve behaviour.
- Firestore full SDK uses persistent streams, memory caching, retry/backoff and offline write queues; Lite does not provide that oracle. These were inspected but not reproduced or exercised. REST errors, retry policy and cache semantics remain incomplete.
- Firestore read tests cover successful documents; missing/permission/network/malformed-response cases need a broader differential corpus. Authenticated Firestore request headers and token-refresh integration need end-to-end differential tests.
- Auth currently accepts Rust strings for credentials, not the entire IPC value domain. Odd values, missing fields, malformed successful responses, missing token expiry/JWT inference, password-policy recaching, and successful reCAPTCHA configuration/browser-verifier failures remain incomplete.
- Auth's event queue is not a full listener implementation: multiple subscriptions, unsubscribe, callback scheduling/concurrency, token listeners and proactive refresh are unverified. There is no process-global app registry implementing repeated initializeApp/getAuth calls.
- Lone-surrogate field names/strings currently pass through lossy UTF-8. Invalid/out-of-range Date, BigInt, cyclic/shared values, other custom IPC objects, timestamp range validation and overlapping update paths need further work. Several unsupported kinds currently use a generic validation error.
- SDK heartbeat generation is pinned to the installed registration/version combination and first request; multi-day heartbeat behaviour is unverified.
- New-module mutation checks have not been performed.

## Validation

`cargo build -q` passed. The offline suites report:

```text
firebase_auth parity: compared 69, differences 0
firestore parity: compared 527, differences 0
lyrics parity: compared 437, differences 0
ytmusic parity: compared 200, differences 0
```

These counts do not close the gaps above. Neither the JavaScript oracle sources nor PORT.md were edited. No git commands were used. cloud-auth.js was not ported or wired to this layer.
