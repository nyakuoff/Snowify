# Step 11a — full-SDK replay update (2026-11-01)

The complete corpus now contains **77 genuine observations**, generated outside
the sandbox by the unmodified full Firebase Firestore SDK 12.11.0 against the
in-process gRPC server. The earlier blocked-corpus and Lite-fallback reports are
superseded. The generator's grpc-js 1.9 server.start() fix is retained; the corpus
was neither regenerated nor edited in this implementation.

**Outcome parity is passing for this corpus; exact wall-clock parity and actual
Electron serialization of DocumentReference are not established.** Do not treat
the passing comparison count as proof that every SDK facility has been ported.

## Production changes

- FirebaseBackend continues to use firebase/firestore.rs and REST exclusively.
  Transient writes retry instead of returning an immediate Lite error. Backoff
  starts with an immediate retry, then 1 second, multiplied by 1.5 up to 60 seconds,
  with the SDK's 0.5–1.5 jitter range. Resource exhaustion starts at the maximum.
  There is no 12-second deadline in production; the caller's pending observation
  window is a test boundary, not cancellation/rejection of the underlying write.
- Read transport failures yield unavailable / “Failed to get document because the
  client is offline.” Application errors inside a batchGet response preserve their
  canonical status and message. Writes preserve canonical status and reproduce the
  observed numeric gRPC prefix, including INVALID_ARGUMENT vs FAILED_PRECONDITION
  (both map to HTTP 400). Lite request prefixes, V8 REST parsing quirks, and
  Lite-specific internal assertion strings were removed from the production path.
- REST requests carry Authorization and the observed resource/client/app metadata.
  Expired-token credential acquisition refreshes and reacquires credentials before
  sending the document request. The gate checks the actual token, refresh ordering,
  all recorded Auth requests, and heartbeat bytes (date taken from the recording).
  Firestore's recorded metadata has no x-firebase-client heartbeat: sign-in already
  consumed it. No heartbeat is invented on those Firestore requests.
- BackendError retains its diagnostic code in its second tuple field. cloud-auth
  still returns only its first field, err.message, as the Electron handler does.
- referenceValue decodes to JsValue::FirestoreReference(path), not a string.
  cloud:load preserves this internal marker. ipc::clone_reply checks nested
  containers before encoding; shell's reply path uses it and rejects with
  “An object could not be cloned.” A separate Node structuredClone diagnostic
  reproduces the corpus's DataCloneError, code 25, and function-cloning message.
  The actual Electron IPC wording is still unmeasured by this corpus.

## Gate and transport boundary

parity_firestore_full is **not ignored**. Each of the 77 operations runs through
the production FirebaseBackend and Backend trait, with offline Host responses;
there are no sockets, real Firebase requests, OS keyring calls or user-data reads.
Auth uses the real Rust Auth implementation, not FakeBackend.

The gRPC corpus distinguishes a failed Listen stream from a target REMOVE/cause.
REST does not expose Listen target events. The replay adapter therefore represents
a failed stream as a non-2xx HTTP response, and a target cause as an error envelope
inside a successful batchGet response stream. The canonical RPC status is preserved
in both. This is an explicit transport translation, **not evidence that an ordinary
HTTP error alone distinguishes stream errors from target errors**. There are no
scenario IDs, fixture messages, or oracle outcomes consulted by production code.

The gate compares all outcomes (including exact rejection message/code and the
12-second pending window), reference summary/Node clone diagnostic, successful and
permanent-error document attempts, metadata, and Auth wire traffic. It skips
pending retry attempt counts because the oracle's jitter was not seeded. Recovery
backoff is asserted within the SDK's 500–1500 ms jitter support. Tokio's
paused clock exercises the real production sleeps without waiting minutes.

parity_firestore remains a Lite **serializer/value-decoder** oracle: 528 comparisons.
Its transport-error and malformed-success cases were trimmed with an explanatory
note. It no longer requires production code to reproduce Lite-only errors.

Additional integrated tests prove:
- cloud:save through CloudAuth and FirebaseBackend stays pending for 12 seconds
  while unreachable (including the migration reads preceding the write).
- cloud:load preserves a DocumentReference until the IPC serialization boundary.
- nested reference markers reject replies; ordinary strings still serialize.

## Validation

From repository root:

```sh
cargo test --offline --manifest-path src-tauri/Cargo.toml -q \
  --test parity_firebase_auth --test parity_firestore \
  --test parity_firestore_full --test parity_cloud_auth -- --nocapture
cargo build --offline --manifest-path src-tauri/Cargo.toml -q
python3 src-tauri/parity/firebase_mutations.py
```

Acceptance gates: **8 Rust tests passed, none ignored**.

```text
firebase_auth parity: compared 93, differences 0
firestore parity: compared 528, differences 0
firestore_full parity: compared 228, differences 0
cloud_auth parity: compared 110, differences 0
```

The offline production build passed. git diff --check passed. No renderer changes,
commits, keyring access, real networking, or edits outside src-tauri/ were made.
Windows compilation/runtime was not available here (the existing uv_errors.rs
platform compile guard is outside this task).

## Remaining gaps — not exceptions to the parity contract

- **Read errors over REST vs Listen (reviewer note, 2026-11-01).** Reads currently map every
  non-OK `batchGet` HTTP status to "Failed to get document because the client is offline.",
  matching the oracle's stream-level failures. Against real Firestore, a security-rules denial
  arrives on the Listen stream as a *target* error ("Missing or insufficient permissions.",
  `permission-denied`), while REST reports the same denial as HTTP 403. So a rules denial
  probably shows "client is offline" in the port but the permission message in Electron. The
  fake server can't settle which real server errors are target-level; that needs real-server
  evidence, which this port does not collect (no real Firebase project in tests).
- **Electron IPC clone failure, confirmed (2026-11-01).** A real Electron 33 probe returning a
  full-SDK DocumentReference from `ipcMain.handle` rejects with `Error invoking remote method
  '<ch>': Error: An object could not be cloned.`, which `ipc::clone_reply` reproduces. Electron
  also logs `Error occurred in handler for '<ch>': …` to stderr; the port doesn't print it for
  any handler error (the terminal-output gap in PORT.md).

1. **Exact elapsed time ±25% is not implemented or asserted.** The recording uses
   real, unseeded SDK jitter. Identical retry policies observed 555 ms for set's
   INTERNAL recovery and 1101 ms for update's INTERNAL recovery: their ±25% bands
   are disjoint. Production reproduces the policy/distribution, not those specific
   random draws. Fast operations also include scheduler/initialization costs absent
   in the in-memory REST replay. The current gate must not be cited as wall-clock
   parity proof. A future oracle must record/control random draws or compare
   distributions across repeated runs; do not silently fit delays to fixture IDs.
2. **REST vs Listen target context:** see the explicit translation above. No
   observed REST server supplies that extra distinction, and a single HTTP status
   cannot recover it. A real streaming Listen transport would be needed for exact
   target/stream delivery parity independent of this translation.
3. **DocumentReference actual Electron IPC:** the corpus measures Node
   structuredClone only. The marker rejects at the shell boundary, but identical
   Electron IPC wording has not been measured. Its summarized reference path/type
   is retained; the SDK's entire app/component object graph is not recreated.
4. Persistent stream/cache semantics, offline write persistence across restart,
   concurrent writes while a pending operation holds the backend auth lock,
   proactive auth refresh/listener unsubscribe/concurrency, and multi-day heartbeat
   behavior remain unverified. There is no full process-global Firebase app registry.
5. Auth malformed successes, missing-expiry/JWT inference, password-policy
   recaching and successful reCAPTCHA/browser-verifier flows remain incomplete.
   Invalid Dates, BigInt/custom inputs, lone-surrogate fields, overlapping update
   paths, timestamp validation and full custom-object behavior need more coverage.

## Mutations

The script rsyncs sources to /tmp/mutws, symlinks node_modules/src/plugins/assets/bin,
touches copied sources only, and recompiles the copied Firebase/cloud modules with
rustc. Already-built dependencies/unrelated modules are read-only inputs; generated
rlibs, test binaries and logs live under /tmp/mutws. The four existing parity
harnesses supply assertions; a baseline must pass before mutations are classified.
Compile errors are not counted as kills. Final run: **30 executed, 22 killed,
8 survived, 0 compile failures** (10 mutations per requested file).

| Source | Killed / executed |
| --- | ---: |
| src/firebase/mod.rs | 6 / 10 |
| src/firebase/firestore.rs | 10 / 10 |
| src/cloud_auth.rs | 6 / 10 |

Survivors are coverage gaps, not declared equivalent:
- Auth: strict vs inclusive expiry-buffer boundary, MFA response rejection,
  needConfirmation rejection, and ignoring updateProfile's returned idToken.
- cloud-auth: missing/null backfill, empty-array backfill, nested-map backfill,
  and migration timestamp.

Machine-readable results and individual logs are retained in
/tmp/mutws/firebase-mutations.json and /tmp/mutws/firebase-*.log. An initial
interrupted attempt lost its read-only baseline rlib during a simultaneous Cargo
rebuild; none of its compile errors were counted. The final run cached baseline
rlibs inside /tmp/mutws and completed all 30 mutations successfully.
