---
name: affine-cli
description: Use this skill whenever the user wants to interact with their AFFiNE workspace — listing documents, searching notes, reading or classifying content, checking analytics, publishing, managing roles, trashing docs, uploading/downloading blobs, or managing access tokens. Triggers on phrases like "my AFFiNE docs", "check the wiki", "search my notes", "delete these pages", "what's in my workspace", "publish this page", "show recent docs", "who has access to", "list blobs", "affine-cli", "run affine". Also triggers on any reference to the user's self-hosted AFFiNE instance (`affine.rosterloh.com`) or workspace operations on it.
---

# AFFiNE CLI

The `affine-cli` binary lives in this repo at `affine-cli/` and wraps the AFFiNE GraphQL API, REST blob storage, and the real-time Socket.IO sync endpoint. Use it whenever the user wants to do anything with their AFFiNE workspace.

## Finding and running the binary

The CLI is a workspace member — build once, then invoke the debug binary:

```bash
cargo build -p affine-cli                                  # build if missing
AFFINE_CLI=/home/rio/src/github/rosterloh/rust_utilities/target/debug/affine-cli
$AFFINE_CLI --help
```

If the user's shell has the crate on PATH, just use `affine-cli` directly. Prefer the compiled binary over `cargo run -p affine-cli -- …` since it avoids recompile noise in the output.

Each Bash invocation in the harness runs in its own shell, so `AFFINE_CLI=…` does not persist between tool calls — either set it at the top of every command block, or just inline the full path. Same applies to loop variables like `$WS` and `$CURSOR`.

## Auth is already configured

The user typically has two env vars set:

- `AFFINE_BASE_URL` — e.g. `https://affine.rosterloh.com`
- `AFFINE_API_TOKEN` — a personal access token (Bearer auth)

With those set, every subcommand just works. Verify with:

```bash
$AFFINE_CLI auth whoami
```

If that fails, check `$AFFINE_CLI config show` and `$AFFINE_CLI auth session`. Session cookies (from `auth login`) live in `~/.config/affine-cli/config.json`.

## Every command prints JSON

All subcommands emit pretty-printed JSON to stdout. For simple field selection `jq` is fine; for anything that touches doc content (summaries, titles from `--resolve`, search highlights) **prefer Python** — AFFiNE's output can contain raw control characters that `jq` refuses to parse.

```bash
# fine for simple field selection
$AFFINE_CLI workspace list | jq '.workspaces[] | {id, role, memberCount}'

# safer for anything involving doc bodies / summaries
$AFFINE_CLI doc list "$WS" --first 50 --resolve \
  | python3 -c 'import json,sys; [print(e["node"]["id"], e["node"].get("title")) for e in json.load(sys.stdin)["workspace"]["docs"]["edges"]]'
```

## The default workspace

The user's primary workspace id is usually available from `workspace list`. When the user says "my AFFiNE" or "the wiki" without specifying, run `workspace list` first and use the single workspace if there is only one, or ask if there are several.

## Command cheat-sheet

### Workspaces
- `workspace list` — all workspaces the user belongs to
- `workspace get <id>` — detail + quota (same quota object as `blob usage`; use whichever is handier)
- `workspace update <id> --enable-ai true --public false` — toggle flags
- `workspace create` / `workspace delete <id>` — manage workspaces

### Documents (pages)
- `doc list <ws> --first 50 [--after <cursor>] [--resolve]`
- `doc recent <ws> --first 20` — ordered by last-updated
- `doc public-list <ws>` — docs shared to the web
- `doc get <ws> <docId>` — title, summary, meta for one doc
- `doc search <ws> "keyword" --limit 20`
- `doc analytics <ws> <docId> [--window-days 30] [--timezone Europe/London]`
- `doc publish <ws> <docId> --mode Page|Edgeless`
- `doc unpublish <ws> <docId>`
- `doc role grant <ws> <docId> --user U1 [--user U2 ...] --role Editor|Reader|Manager|Commenter`
- `doc role update <ws> <docId> --user U --role <DocRole>`
- `doc role revoke <ws> <docId> --user U`
- `doc role default <ws> <docId> --role <DocRole>`
- `doc trash <ws> <docId>` — **destructive**, see gotcha below

### Blobs
- `blob list <ws>`
- `blob head <ws> <key>` — size/mime/etag without downloading
- `blob usage <ws>` — `blobsSize` + quota
- `blob download <ws> <key> --output ./file`
- `blob upload <ws> <path> [--mode auto|graphql|presigned|multipart] [--key <key>] [--mime <type>]`
- `blob delete <ws> <key> [--permanently]`
- `blob release <ws>` — purge soft-deleted blobs
- `blob abort-upload <ws> <key> --upload-id <id>`

### Auth / tokens
- `auth whoami` / `auth session` / `auth sessions`
- `auth token list` / `auth token create --name <n> [--expires-at ISO8601]` / `auth token revoke <id>`
- `auth login <email> --password <pw>` or `--magic-link` then `auth magic-link-confirm`
- `auth verify-email send --callback-url <url>` / `auth verify-email confirm <token>`
- `auth oauth <provider>` — prints the authorize URL
- `auth logout`

### Escape hatch
`$AFFINE_CLI graphql --query <text> [--variables <json>]` or `--query-file <path>`. Use this whenever a feature isn't exposed as a dedicated subcommand: workspace members, invites, comments, Copilot sessions, or the advanced `workspace.search` (Elasticsearch-style) operation. Introspect with:

```bash
$AFFINE_CLI graphql --query '{ __schema { mutationType { fields { name args { name } } } } }'
$AFFINE_CLI graphql --query '{ __type(name:"WorkspaceType") { fields { name args { name } } } }'
```

## Known gotchas — read these before trusting output

1. **`doc list` returns `title: null` / `summary: null`.** The list query reads from the embedding indexer's cache. On self-hosted or freshly-created docs, those fields are empty. Three workarounds, cheapest first:
   - `doc search <ws> "keyword" --limit N` — results come back with titles already populated, so for "find docs about X" this is the shortest path.
   - `doc recent <ws>` — tends to be populated sooner than `doc list`.
   - `doc list --resolve` — follows every id up with a `doc get` call to populate title/summary. Costs one request per doc, so only reach for it when you genuinely need full coverage.

2. **Pagination uses cursors.** Response shape is `{edges: [{cursor, node}], pageInfo: {endCursor, hasNextPage, …}, totalCount}`. Two non-obvious requirements:
   - Use an `if/else` around the CLI call instead of `${CURSOR:+--after "$CURSOR"}`. The `:+` expansion word-splits differently across shells and can surface as `unexpected argument '--after X'`.
   - Write each page straight to a file and parse it from disk. Capturing large JSON via `RESP=$(…)` then `echo "$RESP"` can intermittently fail in agent harnesses (NUL stripping, encoding round-trips), even though the bytes are technically valid.

   ```bash
   : > /tmp/docs.jsonl
   CURSOR=
   while :; do
     if [ -z "$CURSOR" ]; then
       $AFFINE_CLI doc list "$WS" --first 100 > /tmp/page.json
     else
       $AFFINE_CLI doc list "$WS" --first 100 --after "$CURSOR" > /tmp/page.json
     fi
     CURSOR=$(python3 <<'PY'
   import json
   d = json.load(open("/tmp/page.json"))["workspace"]["docs"]
   with open("/tmp/docs.jsonl", "a") as f:
       for e in d["edges"]:
           f.write(json.dumps(e["node"]) + "\n")
   print(d["pageInfo"]["endCursor"] if d["pageInfo"]["hasNextPage"] else "")
   PY
   )
     [ -z "$CURSOR" ] && break
   done
   wc -l /tmp/docs.jsonl
   ```

   `totalCount` in the response can exceed the number of edges you see — trashed / permission-filtered docs are counted but not returned. Don't treat that as a pagination bug.

3. **Tags are not in the API.** AFFiNE stores per-doc tags inside the workspace's YJS blob — they're only reachable through the web UI. Any request involving tag creation/assignment/read should be answered "tags must be managed in the UI; I can still propose a taxonomy from titles and summaries."

4. **`doc trash` intentionally returns `server: null`.** The server runs the delete via the Socket.IO `space:delete-doc` event and broadcasts the update instead of sending an ack, so the CLI treats ack-timeout as success. To *verify* a trash actually happened, run `doc get <ws> <docId>` — a successful trash makes the server reply `DOC_NOT_FOUND`.

5. **`blob upload --mode presigned|multipart` may be downgraded.** On self-hosted servers without object storage, `createBlobUpload` returns `method: "GRAPHQL"`. `--mode auto` falls back transparently; explicit `--mode presigned` or `--mode multipart` errors out with a clear message saying to use `auto` or `graphql`. Default to `--mode auto`.

6. **`blob upload` key = filename.** The legacy GraphQL upload path picks the blob key from the multipart filename, not from `--key`. If you need a specific key, use `--mode presigned` (where supported) or rename the file on disk first.

7. **Rate limits.** Default is 120 req / 60 s; sensitive endpoints are stricter. When iterating over many docs, add a small sleep or batch through `doc recent`/`workspace.searchDocs` instead of hammering `doc get`.

8. **"Empty doc" has no clean API signal.** `DocType` exposes `title`, `summary`, and `mode`, but **no** `contentLength`, `blockCount`, or `children` field, so you cannot reliably tell a brand-new blank page from an intentional section-header page (e.g. Head / Torso / Arms that act as parents) from the list response alone. Treat these cues as *candidates only* and confirm with the user before trashing:
   - **Strong candidate:** `title` is null or visually an id-like string, `summary` is empty, `createdAt ≈ updatedAt` (within seconds), no one else has touched it.
   - **Ambiguous:** `summary` empty but `title` is a real word (often parent/index pages — _do not_ auto-trash).
   Always `--resolve` before classifying, because raw `doc list` nulls are the indexer cache, not real emptiness.

## Common task recipes

### "List every doc with its real title"

Same pagination skeleton as gotcha #2 but with `--resolve` and a richer per-doc projection:

```bash
WS=$($AFFINE_CLI workspace list | python3 -c 'import json,sys; print(json.load(sys.stdin)["workspaces"][0]["id"])')
: > /tmp/docs.jsonl
CURSOR=
while :; do
  if [ -z "$CURSOR" ]; then
    $AFFINE_CLI doc list "$WS" --first 100 --resolve > /tmp/page.json
  else
    $AFFINE_CLI doc list "$WS" --first 100 --resolve --after "$CURSOR" > /tmp/page.json
  fi
  CURSOR=$(python3 <<'PY'
import json
d = json.load(open("/tmp/page.json"))["workspace"]["docs"]
with open("/tmp/docs.jsonl", "a") as f:
    for e in d["edges"]:
        n = e["node"]
        f.write(json.dumps({
            "id":            n["id"],
            "title":         n.get("title"),
            "summary":       n.get("summary"),
            "createdAt":     n.get("createdAt"),
            "updatedAt":     n.get("updatedAt"),
            "mode":          n.get("mode"),
            "public":        n.get("public"),
            "creatorId":     n.get("creatorId"),
            "lastUpdaterId": n.get("lastUpdaterId"),
        }) + "\n")
print(d["pageInfo"]["endCursor"] if d["pageInfo"]["hasNextPage"] else "")
PY
)
  [ -z "$CURSOR" ] && break
done
wc -l /tmp/docs.jsonl
```

Fields available on each node: `id`, `title`, `summary`, `mode`, `public`, `defaultRole`, `createdAt`, `updatedAt`, `creatorId`, `lastUpdaterId`, `workspaceId`. No `contentLength` or `blockCount` — see gotcha #8 for why that matters.

### "Find the doc called X"

```bash
$AFFINE_CLI doc search "$WS" "X" --limit 10 \
  | python3 -c 'import json,sys; [print(r["docId"], "::", r["title"]) for r in json.loads(sys.stdin.read(), strict=False)["workspace"]["searchDocs"]]'
```

`doc search` returns titles populated even when `doc list` doesn't, so reach for it first. If search misses (indexer lag), fall back to listing + `--resolve` and match titles client-side.

### "Clean up the empty docs"

The skill can't decide what "empty" means on its own (see gotcha #8). Two-phase flow: classify, ask, trash.

```bash
# 1. Fully resolved dump of every doc — run the "List every doc with its real title"
#    recipe above so /tmp/docs.jsonl exists.

# 2. Bucket candidates into "very likely empty" vs "needs confirmation".
#    Also fetch the current user so we can check "only I have touched this doc".
ME=$($AFFINE_CLI auth whoami | python3 -c 'import json,sys; print(json.load(sys.stdin)["currentUser"]["id"])')
python3 <<PY
import json, re
from datetime import datetime, timezone

ME = "$ME"
# AFFiNE doc ids are nanoid-ish: ~21 chars, alnum + '-'/'_', mixed case and usually a digit.
NANOID = re.compile(r'^[A-Za-z0-9_-]{18,}\$')
def looks_like_nanoid(s):
    return bool(NANOID.match(s)) and any(c.isdigit() for c in s) and any(c.isupper() for c in s)
def ts(v):
    try: return datetime.fromisoformat(v.replace("Z","+00:00"))
    except Exception: return None
def untouched(d):
    a, b = ts(d.get("createdAt")), ts(d.get("updatedAt"))
    if not a or not b: return False
    return abs((b - a).total_seconds()) <= 5  # within a few seconds = never edited
def only_me(d):
    c, u = d.get("creatorId"), d.get("lastUpdaterId")
    return ME and c == ME and u == ME

strong, maybe = [], []
for line in open("/tmp/docs.jsonl"):
    d = json.loads(line)
    t = (d.get("title") or "").strip()
    s = (d.get("summary") or "").strip()
    if (not t or looks_like_nanoid(t)) and not s and untouched(d) and only_me(d):
        strong.append({"id": d["id"], "title": t or "(null)", "createdAt": d.get("createdAt")})
    elif not s and untouched(d):
        # titled, empty body, never edited — could still be a parent/index page.
        maybe.append({"id": d["id"], "title": t})
print("STRONG CANDIDATES (probably safe to trash):")
for x in strong: print(" ", x)
print(f"\\nNEEDS CONFIRMATION ({len(maybe)} docs, likely section headers or stubs):")
for x in maybe: print(" ", x)
PY

# 3. Show both lists to the user, get explicit approval, THEN trash only the approved ids:
for id in $APPROVED_IDS; do
  $AFFINE_CLI doc trash "$WS" "$id"
  $AFFINE_CLI doc get "$WS" "$id" 2>&1 | grep -q DOC_NOT_FOUND \
    && echo "trashed $id" \
    || echo "trash verify FAILED for $id"
done
```

Signals the bucketer uses (all four must hold for a STRONG candidate):
- `title` is null, empty, or nanoid-like (mixed-case 18+ char blob with a digit — looks like an id, not a word).
- `summary` is empty.
- `updatedAt` is within ~5 s of `createdAt` — nobody has ever typed in it.
- `creatorId == lastUpdaterId == current user` — no other collaborator has touched it.

Docs whose `updatedAt` is far past `createdAt` are *intentionally* dropped from both buckets — someone spent time in them, so the empty summary is indexer lag, not emptiness. Real words as titles — even short ones like `fstab`, `Linux`, `Arms`, `Thor` — almost always mean "intentional parent / index page". Ask, don't trash.

### "How full is the workspace?"

`blob usage` returns two distinct numbers — explain both to the user:

- `blobLimit` — the **per-blob** size cap (on Pro this is 100 MB). A single file bigger than this is rejected.
- `storageQuota` / `usedStorageQuota` — the **total** bytes the workspace is using across all blobs.

```bash
$AFFINE_CLI blob usage "$WS" | python3 -c '
import json, sys
d = json.load(sys.stdin)["workspace"]
q, h = d["quota"], d["quota"]["humanReadable"]
used_mb = d["blobsSize"] / 1_048_576
total_mb = q["storageQuota"] / 1_048_576
print(f"used:  {used_mb:,.1f} MB of {h[\"storageQuota\"]} total ({100*used_mb/total_mb:.2f}%)")
print(f"per-blob limit: {h[\"blobLimit\"]}")
'
```

### "Who can edit this doc?"

No dedicated subcommand — use the raw GraphQL field:

```bash
$AFFINE_CLI graphql --query '
  query($w:String!,$d:String!){ workspace(id:$w){ doc(docId:$d){
    defaultRole
    grantedUsersList(pagination:{first:100}){ edges{ node { user{ id name email } role } } }
  } } }' \
  --variables "{\"w\":\"$WS\",\"d\":\"$DOC\"}"
```

### Advanced search with filters

`doc search` only exposes keyword + limit. For anything richer (boolean/match/boost/exists), use the raw `workspace.search` operation through `graphql`:

```bash
$AFFINE_CLI graphql --query-file search.graphql --variables '{"id":"…","input":{"table":"doc","query":{…},"options":{…}}}'
```

Introspect `SearchInput`, `SearchQuery`, `SearchOptions`, `SearchTable` first to see the available fields.

## When the user asks for something this CLI can't do

- **Tag management** → only via the web UI (YJS-only state).
- **Creating a brand-new doc** → not implemented; the user must create in the UI, then the CLI can manage it.
- **Editing doc content / applying YJS updates by hand** → possible via `applyDocUpdates` but requires a YJS encoder (the CLI doesn't ship one). Point the user at the web app or the raw sync protocol if they really want this.
- **Comments / replies / notifications / Copilot sessions** → available in GraphQL but not wrapped yet; use the `graphql` subcommand with the appropriate mutation name.

## Before invoking a destructive command

Always confirm with the user before running `doc trash`, `workspace delete`, `blob delete --permanently`, `blob release`, `auth token revoke`, or `workspace update --public`. Show the list of ids / the exact command first, wait for the green light, then run it.
