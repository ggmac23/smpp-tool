# Test-only TLS material

These keys and certificates exist so the test suite can drive a TLS SMSC on
localhost. They are committed on purpose and are **not secret**: the private
keys are in this repository in plaintext.

Never use any of this outside the test suite.

- `ca.crt` / `ca.key` — the test CA, which signs the other two.
- `server.crt` / `server.key` — the fixture SMSC. CN `localhost`, SAN
  `DNS:localhost` and `IP:127.0.0.1`.
- `client.crt` / `client.key` — a client certificate, for the mutual-TLS tests.

Generated with a long expiry (2046) so the suite does not start failing on a
date. Regenerate with the `openssl` commands recorded in the Task 8 commit.
