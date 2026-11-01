# Step 11a implementation report — incomplete

This is a tested initial Firebase layer, **not completion of the requested exact-parity port**. Passing counts describe the checked corpus, not all SDK behaviour. Do not start 11b on the assumption that all Firebase semantics are covered.

## API provided

`firebase::Auth` owns one in-memory auth instance. Public methods: `sign_in_with_email_and_password`, `create_user_with_email_and_password`, `send_password_reset_email`, `sign_out`, `update_profile`, `get_id_token`, and `fields`. `current_user` exposes uid, email, display_name and photo_url; tokens remain private. `on_auth_state_changed` registers event capture (including the initial value); `take_events` drains snapshots after operations. This is an event queue, not yet a general asynchronous subscription/unsubscribe API. The cloud_backend adapter owns this singleton and exposes queued events; actual asynchronous listener ordering remains unverified.

`firebase::firestore::{doc,get_doc,set_doc,update_doc}` uses `Document`, `Snapshot::{exists,data}`, and IPC `JsValue`. `set_doc` means merge=true. Writes serialize through `write_body`; reads use `decode_fields`/`decode_value`. Network operations take `Host` and mutable `Auth`, obtaining/refreshing the signed-in token. `Error` exposes code and message.

`FetchRequest` now carries method and optional body. RealHost uses reqwest's request builder. Existing lyrics calls explicitly use GET/no body. MockHost retains legacy fetch-call recording and additionally records complete requests for Auth comparisons. No dependencies added. The existing base64url encoder is reused through crate visibility.

## Oracle and comparisons

Both new runners execute installed Firebase SDK 12.11.0 under Electron's Node. Global fetch is mocked before SDK import; no live Firebase requests are possible. Auth compares method, URL, normalized Headers entries, exact body string, result/error, and callback snapshots. Time is fixed on both sides. Scenarios use separate app names: the SDK's shared inMemoryPersistence survives deleteApp, so reusing the default name leaked state between otherwise independent scenarios. This isolation does not alter the SDK persistence implementation.

Firestore uses the Lite SDK's real parser/serializer. Writes are exercised through Host and their parsed REST bodies and errors compared. Reads now use POST documents:batchGet on both sides and compare parsed request bodies as well as decoded results/errors. This replaces the former Rust GET transport: a missing document is a successful batchGet missing result, while HTTP 404 remains a not-found error. URLs and headers are not yet compared by this Firestore harness. The runner observes decoded object shapes through Node structuredClone, not an actual Electron IPC round trip.

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

## Priority 1 follow-up — closed with the permitted Lite fallback

The full SDK installed here uses `@grpc/grpc-js`, not fetch/WebChannel in Node. Evidence: `node_modules/@firebase/firestore/dist/common-882bbb15.node.cjs.js`, `GrpcConnection.createMetadata`, `openStream`, and `invokeStreamingRPC`. A local `connectFirestoreEmulator` server cannot run in this sandbox: the following offline capability probe exited 1 with `listen EPERM: operation not permitted 127.0.0.1`:

```sh
ELECTRON_RUN_AS_NODE=1 node_modules/.bin/electron -e 'const n=require("net").createServer();n.on("error",e=>{console.error(e);process.exit(1)});n.listen(0,"127.0.0.1",()=>{console.log(n.address());n.close()})'
```

No SDK implementation was modified or grpc methods mocked to manufacture full-SDK results. The permitted fallback uses the installed `firebase/firestore/lite` parser/transport under Electron with offline fetch replay, and Rust with MockHost. No new crates, Firebase project access, real network traffic or OS keyring calls were used.

`parity_firestore` adds 247 cases (527 → 774): for each of getDoc, setDoc(merge) and updateDoc, 15 HTTP statuses with structured/array/missing errors, five non-JSON bodies at five statuses, network failure, five malformed successful response shapes, and six non-string/empty/null error messages; plus a successful missing document. Conflicting JSON error.status proves the SDK maps HTTP status instead. Error messages are compared verbatim, including V8 SyntaxError text, native errors without a code, statusText fallback, and JavaScript coercion. The production backend forwards Firebase Error.message through `cloud_backend::fail`; existing cloud-auth handler differential coverage remains 110 cases, but is a mocked-backend suite, not a new integrated full-SDK test.

**Messages/behavior that may differ from the app's full SDK:** Lite's `Request failed with error: ...` prefix versus gRPC status-prefixed errors; full-SDK offline getDoc errors (`Failed to get document because the client is offline.`); retryable unauthenticated/network/write failures that may stay pending or retry rather than reject immediately; malformed gRPC payload errors versus REST JSON SyntaxErrors; Lite-specific malformed-success internal assertion messages. These are explicitly fallback parity, not evidence of full-SDK equivalence.

Stopping after priority 1 under the requested budget clause. Priorities 2–5 were not attempted in this follow-up.

## Full-SDK AF_UNIX follow-up — blocked, generator ready

The suggested Unix-domain transport was probed before attempting implementation. Electron's Node `net.createServer().listen('/tmp/<unique>/fs.sock')` also fails with `EPERM: listen EPERM: operation not permitted`. grpc-js `Server.bindAsync('unix:/tmp/<unique>/fs.sock', ...)` was attempted separately and reports `No address added out of total 1 resolved`. The generator now performs the direct AF_UNIX preflight so its corpus records the underlying EPERM rather than grpc-js's generic wrapper message.

Added `parity/node/firestore_full.js`, `parity/corpus/firestore_full.json`, and `tests/parity_firestore_full.rs`. The corpus currently has **status=blocked, 77 scenario specifications, results=[]**. There are zero observed full-SDK outcomes. Neither Lite nor Rust results are substituted as full-SDK evidence.

The runner reads the installed full SDK's bundled protobuf literal and loads it with the same `@grpc/proto-loader.fromJSON` options as `loadProtos()`. SDK implementation functions remain unmodified. A local grpc-js server implements BatchGetDocuments, Commit, Write and Listen; the actual cloud-auth call shapes are getDoc, setDoc(merge=true) and updateDoc. Each operation gets a fresh app/database and a unique local socket, preventing cache/persistence leakage. Auth fetch is restricted to three offline replay endpoints; unexpected fetch URLs fail closed. There is no keyring or real user-data access.

The 77 scenarios cover:

- Success, offline, signed-in and expiry-forced-refresh variants for each operation.
- All nine requested gRPC statuses with custom messages, persistently and twice-then-success, for all three operations. Write failures occur after a successful handshake, so permanent write errors can be surfaced rather than mistaken for handshake retries.
- Nine additional Listen target REMOVE/cause errors, distinguished from gRPC stream failures.
- Missing documents and a full-SDK referenceValue read, including a safe summary of the DocumentReference and structuredClone result/error.

Outputs include outcome (fulfilled/rejected/pending), exact message/code, stream/open counts, individual document requests, metadata received by the server, Auth fetch requests and refresh order, and elapsed wall time. Timing uses **bounded real waits of 12 seconds per operation**, not fake timers; `elapsedVirtualMs` is explicitly null. SDK backoff/jitter is unmodified. The worst-case observation time is about 16 minutes for all scenarios, usually less; do not interrupt merely because a retryable write remains pending. The file is checkpointed after each scenario; partial/failed recordings cannot pass the replay gate. The reference probe is Node structuredClone of actual full-SDK read data, **not an actual Electron IPC round trip**.

Run from the repository root **outside the sandbox**:

```sh
ELECTRON_RUN_AS_NODE=1 node_modules/.bin/electron src-tauri/parity/node/firestore_full.js
```

This overwrites the blocked corpus with observed results. `--only=get-success` can diagnose the server setup, but produces a partial corpus and must be followed by the full command. The generated corpus must be brought back before implementing the Rust full-SDK behavior.

The Rust replay gate consumes the corpus, translates scenario gRPC failures to equivalent REST responses through MockHost, and compares outcomes, document-attempt counts, selected auth metadata and refresh-before-request. It rejects incomplete corpora. It is explicitly **ignored by default pending the external recording**:

```sh
cd src-tauri
cargo test -q --test parity_firestore_full -- --ignored --nocapture
```

This is an unfinished parity gate: with genuine results it is expected to expose the existing Lite-vs-full gaps until the Rust port is corrected. It does not assert exact elapsed time (real jitter is nondeterministic), reproduce target/stream-specific retry policy, or claim the new IPC marker is implemented. `firestore.rs`, `cloud_backend.rs` and `ipc.rs` were deliberately left unchanged without oracle observations, following the requested stop-on-AF_UNIX-denial path. Mutation checks were not started.

Validation: `node --check` passed. `firestore_full.js --self-test` passed all 77 scenario fixture/request/reply encoding checks through the SDK proto serializers and fake stream handlers without sockets. This is fixture validation, not an SDK differential run. Existing acceptance suites still report firebase_auth 93/0, firestore 774/0 and cloud_auth 110/0; the new Rust replay test compiles and reports one explicit ignore. No commits or edits outside src-tauri/ were made in this follow-up.

## Remaining implementation and verification gaps

- DocumentReference decoding is explicitly unimplemented. A probe of Lite referenceValue followed by structuredClone fails with `container => new PlatformLoggerServiceImpl(container) could not be cloned.` An actual full-SDK Electron IPC observation and suitable representation through the cloud-auth layer are still required. Returning a string would not preserve behaviour.
- Firestore full SDK uses persistent streams, memory caching, retry/backoff and offline write queues; Lite does not provide that oracle. These were inspected but not reproduced or exercised. The REST error corpus is now covered by the Lite fallback above; full-SDK error delivery, retry policy and cache semantics remain incomplete.
- Priority 2: authenticated Firestore request headers (Authorization, x-goog-* and heartbeat) and token-refresh integration still need end-to-end differential tests. Request body parity alone does not prove these.
- Credentials already accept &JsValue, including odd IPC values (firebase_auth 93/0); that earlier gap is closed. Malformed successful Auth responses, missing token expiry/JWT inference, password-policy recaching, and successful reCAPTCHA configuration/browser-verifier failures remain incomplete.
- Auth's event queue is not a full listener implementation: multiple subscriptions, unsubscribe, callback scheduling/concurrency, token listeners and proactive refresh are unverified. There is no process-global app registry implementing repeated initializeApp/getAuth calls.
- Lone-surrogate field names/strings currently pass through lossy UTF-8. Invalid/out-of-range Date, BigInt, cyclic/shared values, other custom IPC objects, timestamp range validation and overlapping update paths need further work. Several unsupported kinds currently use a generic validation error.
- SDK heartbeat generation is pinned to the installed registration/version combination and first request; multi-day heartbeat behaviour is unverified.
- Priority 5: mutation checks have not been performed. No mutants were applied anywhere in this follow-up (including the real tree).

| Requested mutation scope | Executed | Killed |
| --- | ---: | ---: |
| src/firebase/*.rs | 0 | not run |
| src/cloud_auth.rs | 0 | not run |
| src/ytm/*.rs | 0 | not run |
| src/ytmusic.rs | 0 | not run |

Priorities 3 (full-SDK DocumentReference IPC rejection/marker) and 4 (single listener asynchronous delivery and ordering relative to awaited operations) remain open as described above.

## Validation

Follow-up acceptance command, with `-- --nocapture` to display counts:

```sh
cargo test -q --test parity_firebase_auth --test parity_firestore --test parity_cloud_auth -- --nocapture
```

```text
firebase_auth parity: compared 93, differences 0
firestore parity: compared 774, differences 0
cloud_auth parity: compared 110, differences 0
```

All tests passed (cloud-auth also includes its safe-storage isolation test). `node --check src-tauri/parity/node/firestore.js` passed; touched Rust files were rustfmt-formatted. The Rust test build verifies compilation and references. Earlier unrelated lyrics/ytmusic counts were 437/0 and 200/0; they were not rerun in this follow-up. No commits were made and no files outside src-tauri were edited by this follow-up.
