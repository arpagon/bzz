# v0.7.1 relay-avatar validation

**Status:** Candidate; not published.

## Automated boundary coverage

`tests/media_test.rs` covers the authenticated relay-avatar branch with a
single-use local server and generated PNG bytes. It proves that bzz:

- accepts only the active relay's canonical lowercase-hash image path;
- rejects external origins, query-bearing paths, unsupported extensions,
  uppercase hashes, and relative profile values before selecting that branch;
- sends a signed `kind 24242`, `t=get`, hash-bound authorization to the
  accepted relay path;
- routes a canonical relay avatar through the graphics media runtime and
  accepts correctly addressed image bytes; and
- rejects and removes bytes that disagree with the content address.

The existing avatar URL, redirect, DNS-public-address, MIME/magic, decoding,
cache-isolation, locked/cache-only, off, and non-graphics tests remain in the
full suite.

## Operator smoke

Using a disposable owner-only destination, the current active community
identity successfully fetched a relay-hosted kind-0 profile image that requires
read authorization. The response was `200`, was 3,748,425 bytes, and was
removed after verification. No profile URL, authorization value, image bytes,
or private cache data is recorded here.

## Required release checks

Before publication, run the locked formatting, strict Clippy, full test, deny,
audit, and release-binary gates. Perform the graphics-terminal scenario in
[`e2e-manual.md`](e2e-manual.md), including a real authenticated relay avatar,
and re-run the pinned real-relay integration suite.
