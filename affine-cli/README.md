# AFFiNE Cli

CLI for the [AFFiNE](https://affine.pro) GraphQL API, REST blob storage, and real-time
sync endpoint. Supports session-cookie login and personal access tokens.

## Installation

```bash
cargo build -p affine-cli --release
./target/release/affine-cli --help
```

## Authentication

```bash
# Password sign-in (session cookies are saved to $XDG_CONFIG_HOME/affine-cli/config.json)
affine-cli --server https://affine.example.com auth login you@example.com --password 'secret'

# Magic-link sign-in: request an e-mail, then exchange the OTP
affine-cli auth login you@example.com --magic-link
affine-cli auth magic-link-confirm you@example.com <token-from-email>

# OAuth — print the authorize URL so you can open it in a browser
affine-cli auth oauth google --callback-url https://example.com/cb

# E-mail verification
affine-cli auth verify-email send --callback-url https://example.com/verified
affine-cli auth verify-email confirm <token>

# Session inspection
affine-cli auth whoami
affine-cli auth session
affine-cli auth sessions

# Personal access tokens (create once, paste into `--token` / AFFINE_API_TOKEN)
affine-cli auth token create --name "ci"
affine-cli auth token create --name "ci-limited" --expires-at 2027-01-01T00:00:00Z
affine-cli auth token list
affine-cli auth token revoke <id>
```

You can also authenticate non-interactively:

```bash
AFFINE_BASE_URL=https://affine.example.com \
AFFINE_API_TOKEN=your_access_token \
affine-cli auth whoami
```

## Workspaces

```bash
affine-cli workspace list
affine-cli workspace get <workspace-id>
affine-cli workspace create
affine-cli workspace update <id> --enable-ai true --public false
affine-cli workspace delete <id>
```

## Documents

```bash
# Browsing
affine-cli doc list <workspace-id> --first 20
affine-cli doc list <workspace-id> --resolve                 # populate title/summary per doc
affine-cli doc recent <workspace-id> --first 10              # recentlyUpdatedDocs
affine-cli doc public-list <workspace-id>                    # docs published to the web
affine-cli doc get <workspace-id> <doc-id>
affine-cli doc search <workspace-id> "keyword" --limit 20    # see below for advanced search

# Analytics
affine-cli doc analytics <workspace-id> <doc-id> --window-days 30 --timezone Europe/London

# Publishing
affine-cli doc publish   <workspace-id> <doc-id> --mode Page     # or --mode Edgeless
affine-cli doc unpublish <workspace-id> <doc-id>

# Roles
affine-cli doc role grant   <workspace-id> <doc-id> --user U1 --user U2 --role Editor
affine-cli doc role update  <workspace-id> <doc-id> --user U1 --role Reader
affine-cli doc role revoke  <workspace-id> <doc-id> --user U1
affine-cli doc role default <workspace-id> <doc-id> --role Manager

# Trash (uses Socket.IO / space:delete-doc — no REST equivalent)
affine-cli doc trash <workspace-id> <doc-id>
```

### Why `doc list` sometimes returns `title: null`

The list query returns whatever AFFiNE's document-embedding indexer has cached. On
self-hosted servers, or for docs that were just created, those fields can be null.
Use `--resolve` to follow each id up with `doc get`, which pulls the title/summary
straight from the doc's root block.

### Advanced search

`doc search` exposes the public `searchDocs` operation, which takes only a keyword
and a limit. The richer `workspace.search` operation (Elasticsearch-style boolean /
match / boost / exists queries) is reachable through the raw `graphql` subcommand:

```bash
affine-cli graphql --query-file my_search.graphql \
  --variables '{"id":"<ws>","input":{"table":"doc","query":{...},"options":{...}}}'
```

## Blobs

```bash
affine-cli blob list     <workspace-id>
affine-cli blob head     <workspace-id> <key>                 # size, mime, etag, last-modified
affine-cli blob usage    <workspace-id>                        # blobsSize + quota
affine-cli blob download <workspace-id> <key> --output ./file

# Uploads. `auto` chooses between GraphQL and presigned PUT based on file size (5 MiB
# threshold). `graphql` forces the legacy single-shot mutation; `presigned` uses the
# new `createBlobUpload` + PUT flow; `multipart` chunks large files into presigned
# part uploads. The server can downgrade presigned/multipart back to graphql when
# object storage isn't configured — `auto` handles that transparently.
affine-cli blob upload   <workspace-id> ./image.png                 # defaults to --mode auto
affine-cli blob upload   <workspace-id> ./big.mp4 --mode multipart --key "videos/intro.mp4"

# Cleanup
affine-cli blob abort-upload <workspace-id> <key> --upload-id <id>  # abort in-flight multipart
affine-cli blob delete  <workspace-id> <key>                         # soft delete
affine-cli blob delete  <workspace-id> <key> --permanently           # hard delete
affine-cli blob release <workspace-id>                               # purge soft-deleted blobs
```

## Raw GraphQL

Anything not covered by a dedicated subcommand (members, invites, comments, Copilot
sessions, advanced search, etc.) is reachable through the generic `graphql` command:

```bash
affine-cli graphql \
  --query 'query q($id: String!) { workspace(id: $id) { members(skip: 0, take: 20) { id email name } } }' \
  --variables '{"id":"<workspace-id>"}'

affine-cli graphql --query-file ./some-query.graphql --variables '{"foo":"bar"}'
```

## Config

```bash
affine-cli config show             # print config path and stored session
affine-cli config clear-session    # forget cookies (keeps base URL)
```

## Links

- [API Overview](https://mintlify.wiki/toeverything/AFFiNE/api/overview)
- [Authentication](https://mintlify.wiki/toeverything/AFFiNE/api/authentication)
- [GraphQL — Documents](https://mintlify.wiki/toeverything/AFFiNE/api/graphql/documents)
- [Blob Storage](https://mintlify.wiki/toeverything/AFFiNE/api/storage/blobs)
- [Real-time Sync](https://mintlify.wiki/toeverything/AFFiNE/api/storage/sync)
