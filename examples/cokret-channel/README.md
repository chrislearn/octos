# Cokret channel for octos

Connects the octos agent to a [Cokret](https://www.cokret.com) v1 server so it
can read and write messages in Cokret realms — the same integration shape as
the savfox bridge. Build with the `cokret` feature:

```bash
cargo build -p octos-cli --features cokret
# or, with the canonical channel set:
cargo install --path crates/octos-cli --features "api,cokret"
```

Add a channel entry to your gateway config's `channels` array (see
[`gateway.example.json`](./gateway.example.json)). Two modes are supported, both
selected by the `mode` setting.

## Account mode (`mode: "account"`)

The agent logs in as one or more **already-existing** Cokret accounts and talks
in the realms those accounts belong to. This is the fastest path to get the
agent speaking.

Required per channel: `baseUrl` (the Cokret server). Required per account:
`principalId` (the account DID), `deviceId`, and either `accessToken`
(a pre-issued `ck.session.grant` bearer) **or** a `keyRef` for DID-proof login.
An account with `send: true` must declare a `defaultRealmId`.

Outbound replies route back to the realm/flow the inbound message came from
(the routing `chat_id` carries `realm_id|flow_id`); when a reply carries no
flow, the account's `defaultFlowId` is used.

### Static bearer vs DID-proof

The example uses a static `accessToken`. Production servers reject unsigned
events, so for a real deployment configure a signing key instead:

```json
"keyRef": { "kind": "env", "var": "OCTOS_COKRET_BOT_KEY" },
"loginChallenge": "server-issued-challenge-at-least-16-chars",
"cokretServerDid": "did:webvh:your-server"
```

With `keyRef` set, octos runs `login_did_proof` at startup and signs every
outbound event with a detached JWS. The 32-byte ed25519 seed is read as
base64-no-pad from the env var (or a file, via `{ "kind": "file", "path": ... }`).

### Capability grant

To carry an `authorization_ref` on outbound writes, point `grantEventPath` at a
pre-signed `ck.capability.grant` Event JSON issued by a realm admin. It is
verified (proof binding, subject, realm, expiry) before its `event_id` is used.

## Applet mode (`mode: "applet"`)

Registers octos as a Cokret Applet (the Matrix-AppService equivalent). octos
hosts the inbound HTTP endpoints under `/_cokret/edge/applet/...` on `bind_addr`
and replies as the applet **bot** actor (`botActorId`).

The Cokret server pushes events via `POST /_cokret/edge/applet/transactions`
(bearer-authenticated against `accessToken`, deduplicated by `Idempotency-Key`).
Events whose realm matches the declared `namespaces.realms` are dispatched to
the agent; the agent's reply is written back as a signed `ck.message.create`
attributed to the bot, with a restart-safe monotonic `actor_seq`.

Required: `appletId`, `serviceDid`, `controllerDid`, `baseUrl`, `botActorId`,
`cokretServerUrl`, at least one `namespaces` axis, a non-empty `protocols`, and
an `accessToken` for inbound auth. Set `keyRef` (plus `cokretServerDid` +
`loginChallenge`) to sign outbound events; otherwise the static
`cokretServerUrl` bearer is used.

## Local testing

Point `baseUrl` / `cokretServerUrl` at a dev `soland` (`http://127.0.0.1:8008`).
You need a real account DID with write access to the target realm; account mode
with a static `ck.session.grant` bearer is the quickest way in.
