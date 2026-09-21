<div align="center">

# mcp-gryzzly

**Your timesheet, filled by asking.**

A single Rust binary that puts the whole [Gryzzly](https://www.gryzzly.io) time-tracking API
in front of any MCP client — and makes logging time the easy part.

[![Rust](https://img.shields.io/badge/Rust-1.88%2B-CE422B?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![MCP](https://img.shields.io/badge/MCP-2025--11--25-8A63D2)](https://modelcontextprotocol.io)
[![Gryzzly API](https://img.shields.io/badge/Gryzzly%20API-50%2F50%20endpoints-1FB770)](https://api.gryzzly.io/v1/doc/index.html)
[![Transports](https://img.shields.io/badge/transports-stdio%20%2B%20Streamable%20HTTP-0b7285)](#run-it-over-http-instead)
[![CI](https://github.com/edouard-claude/mcp-gryzzly/actions/workflows/ci.yml/badge.svg)](https://github.com/edouard-claude/mcp-gryzzly/actions/workflows/ci.yml)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Stars](https://img.shields.io/github/stars/edouard-claude/mcp-gryzzly?style=flat&logo=github&color=f5c518)](https://github.com/edouard-claude/mcp-gryzzly/stargazers)

</div>

---

Ask your assistant *"log 2h30 on Alyxa yesterday"*, *"what did I miss last week?"*,
*"fill my timesheet from my commits"* — and have it land in the same Gryzzly your
team actually reports on.

```console
> log 2h30 on Alyxa yesterday, pairing on the importer

  log_time      2h30 on Alyxa / Development, Thursday 2026-09-17
                that day now totals 7h30

> did I miss anything last week?

  timesheet     2026-W38 · 31h30 over 5 days
                .INTERNE   16h40  47.6%
                Alyxa       9h00  28.6%
                Le14        5h50  23.8%
                Friday 2026-09-18 is short: 4h of the 7h expected
```

No identifiers, no seconds, no date arithmetic. You name a project, say `2h30`, and
the server resolves the rest.

## Why

Gryzzly's API is an RPC surface of 50 actions where time is attached to tasks by
UUID, durations are seconds, and a list that matches nothing answers `404`. It is a
fine API to integrate and a miserable one to hand a language model raw. This server
is the translation layer.

- **The whole API, nothing hidden.** All 50 documented actions, plus `whoami`,
  `log_time_bulk` and `timesheet`, which exist because the API has no `/me`, no
  batch and no aggregate. A test fails if that count ever drops.
- **Names, not UUIDs.** Every tool takes a project, task, user or customer by name,
  code, email or id. A name that matches two objects comes back with the candidates
  rather than a guess — the one thing you never want automated is *which* project
  your afternoon went to.
- **Durations and dates as people say them.** `2h30`, `90m`, `1.5h`, `2:30`;
  `today`, `yesterday`, `monday`, `2026-W38`, `last week`.
- **Written for a model to read.** Structured output with schemas on all 53 tools,
  annotations on every one, three prompts that stop and ask before writing, and
  server instructions that say up front that time hangs on tasks.
- **Fail-closed where it matters.** Deletes refuse to run without `confirm: true`.
  `GRYZZLY_READ_ONLY=1` withholds every write tool. `GRYZZLY_TOOLSETS` trims the
  surface to what you actually use.
- **One binary, no state.** No database, no OAuth dance, no background jobs. An API
  key in the environment, stdio or HTTP, and Gryzzly's own rate limit respected from
  the client side.

## Install

<details open>
<summary><b>From source (any platform with Rust 1.88+)</b></summary>

```bash
git clone https://github.com/edouard-claude/mcp-gryzzly
cd mcp-gryzzly
cargo build --release
# the binary is at target/release/mcp-gryzzly
```
</details>

<details>
<summary><b>With cargo install</b></summary>

```bash
cargo install --git https://github.com/edouard-claude/mcp-gryzzly
```
</details>

<details>
<summary><b>With Docker</b></summary>

```bash
docker build -t mcp-gryzzly .
docker run --rm -i -e GRYZZLY_API_KEY=… mcp-gryzzly
```

The image runs unprivileged and speaks stdio by default; add `--http` for the HTTP
transport. See [Run it over HTTP instead](#run-it-over-http-instead).
</details>

Get an API key from Gryzzly: **Administration → API keys**. Then check the wiring
before pointing a client at it:

```bash
GRYZZLY_API_KEY=… mcp-gryzzly --check
```

```console
base url     https://api.gryzzly.io/v1
toolsets     time, projects, directory, leaves, budget, exports
read only    false
tools        53
team         6 users visible
acting as    Edouard CLAUDE <edouard@squirrel.fr>
projects     41 (41 active)

ready.
```

## Configure

| Variable | Required | Default | What it is |
|---|:-:|---|---|
| `GRYZZLY_API_KEY` | yes | | The API key, sent as `Authorization: Bearer …` |
| `GRYZZLY_API_KEY_FILE` | | | A file holding the key instead, for Docker secrets |
| `GRYZZLY_USER_ID` | | | The UUID of the person the server acts as |
| `GRYZZLY_USER_EMAIL` | | | That person's email, resolved to an id at startup |
| `GRYZZLY_BASE_URL` | | `https://api.gryzzly.io/v1` | API root, prefix included |
| `GRYZZLY_DEFAULT_PROJECT` | | | A project name, code or id used when a tool omits one |
| `GRYZZLY_TOOLSETS` | | `all` | `time,projects,directory,leaves,budget,exports` |
| `GRYZZLY_READ_ONLY` | | `0` | `1` withholds every write tool |
| `GRYZZLY_TIMEOUT_SECS` | | `30` | Per-request timeout |
| `GRYZZLY_CACHE_TTL_SECS` | | `60` | How long name lookups are reused |
| `GRYZZLY_MAX_RETRIES` | | `3` | Retries on 429 and transient 5xx |
| `GRYZZLY_LOOKBACK_DAYS` | | `365` | How far back a declaration is searched by id |
| `GRYZZLY_HTTP_BIND` | | | Listen address for the HTTP transport |
| `GRYZZLY_HTTP_TOKEN` | | | Bearer token the HTTP transport requires |
| `GRYZZLY_LOG` | | `info` | `warn`, `debug`, `mcp_gryzzly=debug`… |

**One key, one person.** A Gryzzly API key belongs to a *team*, not to a user, and
the API has no `/me`. So `GRYZZLY_USER_ID` or `GRYZZLY_USER_EMAIL` names whose
timesheet is being filled. A team of exactly one user needs neither. The binary
refuses to start on a malformed variable and names the one at fault.

## Connect a client

<details open>
<summary><b>Claude Code</b></summary>

```bash
claude mcp add gryzzly --scope user \
  --env GRYZZLY_API_KEY=… \
  --env GRYZZLY_USER_EMAIL=you@example.com \
  -- /absolute/path/to/mcp-gryzzly
```
</details>

<details>
<summary><b>Claude Desktop</b></summary>

`claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "gryzzly": {
      "command": "/absolute/path/to/mcp-gryzzly",
      "env": {
        "GRYZZLY_API_KEY": "…",
        "GRYZZLY_USER_EMAIL": "you@example.com"
      }
    }
  }
}
```
</details>

<details>
<summary><b>Cursor, Windsurf, Zed, and other stdio clients</b></summary>

Same shape: one `command` pointing at the binary, the key in `env`. The server speaks
the stdio transport and prints nothing but protocol on stdout — logs go to stderr.
</details>

<details>
<summary><b>Any client, over HTTP</b></summary>

Point it at `http://127.0.0.1:8080/mcp` after starting the server with
`--http 127.0.0.1:8080`. See [below](#run-it-over-http-instead).
</details>

Then ask *"who am I in Gryzzly?"*. `whoami` should answer with your name, your role,
and this week's total.

## Tools

53 tools in six toolsets. `GRYZZLY_TOOLSETS` picks which ones are registered;
`GRYZZLY_READ_ONLY=1` keeps only the ones that declare themselves read-only.

### time — the point of the whole thing

| Tool | |
|---|---|
| `whoami` | Who the server acts as, what it may do, and this week's total |
| `log_time` | Declare time: a duration, a task or project name, a day |
| `log_time_bulk` | A whole week in one call, with one outcome line per entry |
| `list_time_entries` | Declared time over a week or a period, names filled in |
| `timesheet` | Totals per day and per project, plus the days that fall short |
| `update_time_entry` | Change a declaration's duration, day, task or description |
| `delete_time_entry` | Remove one, with `confirm: true` |

`log_time` is the one to know. `{"project": "Alyxa", "duration": "2h30", "date":
"yesterday", "description": "pairing on the importer"}` is enough: the project is
matched by name, its single declarable task is chosen (or you name one), the duration
becomes seconds, and the answer repeats the day's new total so a double entry is
visible immediately.

### projects — the catalogue, and its administration

| Tool | |
|---|---|
| `list_projects` | Filtered by status, customer, or a fragment of name or code |
| `get_project` | One project in full |
| `list_tasks` | The tasks of a project, containers and completed ones filtered out |
| `get_task` | One task in full |
| `get_project_metrics` · `list_projects_metrics` | Budget and time counters |
| `create_project` · `update_project` · `delete_project` | Project administration |
| `create_task` · `update_task` · `delete_task` | Task administration |

### directory — people and labels

| Tool | |
|---|---|
| `list_users` · `get_user` · `invite_user` | The team |
| `list_customers` · `get_customer` · `create_customer` · `update_customer` · `delete_customer` | Customers |
| `list_groups` · `get_group` · `list_groupsets` · `get_groupset` | Groups (read-only upstream) |
| `list_tags` · `get_tag` · `list_tagsets` · `get_tagset` | Tags (read-only upstream) |

### leaves — days off

| Tool | |
|---|---|
| `list_leave_periods` · `get_leave_period` | Read |
| `create_leave_period` · `update_leave_period` · `delete_leave_period` | Write |
| `import_leave_periods` | Feed Gryzzly a CSV or XLSX, inline or from a path |

### budget — what a project costs beyond time

| Tool | |
|---|---|
| `list_purchases` · `get_purchase` · `create_purchase` · `update_purchase` · `delete_purchase` | Purchases |
| `list_discounts` · `get_discount` · `create_discount` · `update_discount` · `delete_discount` | Discounts |

Gryzzly gates both behind its budget offer; without it every call answers
`no_permission`, and the error says so.

### exports — the two download endpoints

| Tool | |
|---|---|
| `export_projects` | Gryzzly's project export, CSV or XLSX |
| `export_times_and_leaves` | Declarations and leaves, CSV or XLSX |

Both are exposed for completeness and both currently fail with an API key: see
[Gryzzly, as it actually behaves](#gryzzly-as-it-actually-behaves).

## Resources and prompts

Four resources and two templates, for clients that prefer attaching context to
calling a function:

| URI | |
|---|---|
| `gryzzly://me` | The current user and their week |
| `gryzzly://projects` | Every project |
| `gryzzly://users` | Every user the key can see |
| `gryzzly://customers` | Every customer |
| `gryzzly://projects/{project}/tasks` | The tasks of one project, by name or id |
| `gryzzly://timesheet/{week}` | One week, totalled — `2026-W38`, `this week` |

Three prompts drive the workflows, and all three read before they write:

- **`fill_my_week`** — read the week, propose the missing entries from real evidence,
  show a table, **stop and wait**, then write it in one `log_time_bulk` call.
- **`weekly_review`** — totals, per-project split, and what looks wrong. Writes nothing.
- **`catch_up_timesheet`** — find the empty working days over a period and close them
  one approved week at a time, leave periods excluded.

## Gryzzly, as it actually behaves

Everything below was found against the live API, and every line of it is handled for
you. It is written down because the next person integrating Gryzzly will hit the same
walls.

| What happens | What this server does |
|---|---|
| A `*.list` action matching nothing answers **`404 not_found`**, not an empty page | Reads that 404 as the empty page it means, so "no time logged last week" is not an error |
| `declarations.list` needs `task_ids` **present**, even as `[]`, or it 404s | Always sends both `user_ids` and `task_ids`, empty when unfiltered |
| There is no `/me`: a key is a team key | `GRYZZLY_USER_ID` / `GRYZZLY_USER_EMAIL`, resolved once and cached; a team of one needs neither |
| There is no `declarations.get`, and no id filter on the list | `update_time_entry` finds the entry over `GRYZZLY_LOOKBACK_DAYS` and fills the fields you left out |
| `declarations.update` requires date, task **and** duration on every call | Reads the current values first, so a partial update stays partial |
| `*.export` authenticate a *browser session*, so with a key they answer `500 no cookie` | Says exactly that, and points at `list_time_entries` for the same data |
| Purchases and discounts answer `no_permission` without the budget offer | Surfaces `error_type` in the error's `data`, so a model can tell policy from breakage |
| `users.get` on an unknown id answers `500`, not `404` | Reported as a configuration problem when it is `GRYZZLY_USER_ID` |
| 50 requests per 10 seconds, per team | A sliding window paces the process; 429 and transient 5xx are retried with backoff, honouring `Retry-After` |
| `limit` caps at 1000 | Walked page by page when more is needed |

## Safety

Filling a timesheet is a write operation on the record your employer bills from, so
the defaults lean careful.

- **Deletes are fail-closed.** Seven tools can destroy something; each refuses
  without `confirm: true` and says what would go. They are annotated
  `destructiveHint`, so a client can warn before running them.
- **Read-only is structural.** `GRYZZLY_READ_ONLY=1` drops every route that does not
  annotate itself `readOnlyHint` — not a list of names to keep in sync, so a new write
  tool is excluded by construction. A test asserts it.
- **Nothing is guessed.** An ambiguous name is an error with candidates. A malformed
  duration is refused before any HTTP call. `log_time` reports the day's new total
  rather than letting a duplicate pass unnoticed.
- **The key never leaks.** It is redacted in `Debug`, marked sensitive on the HTTP
  header, and never serialized into a tool answer or a log line.
- **The prompts ask first.** All three stop for explicit approval before writing, and
  the one that proposes entries is told not to invent what it cannot see.

## Run it over HTTP instead

```bash
mcp-gryzzly --http 127.0.0.1:8080
# MCP at http://127.0.0.1:8080/mcp, health at /healthz
```

Off loopback, the server **refuses to start** without `GRYZZLY_HTTP_TOKEN`: one API
key means one identity, and an open endpoint is an open proxy to your team's
timesheet. With the token set, every request to `/mcp` needs
`Authorization: Bearer <token>`; `/healthz` stays open for probes.

```bash
docker run -d --name mcp-gryzzly -p 8080:8080 \
  -e GRYZZLY_API_KEY=… \
  -e GRYZZLY_USER_EMAIL=you@example.com \
  -e GRYZZLY_HTTP_BIND=0.0.0.0:8080 \
  -e GRYZZLY_HTTP_TOKEN="$(openssl rand -hex 32)" \
  mcp-gryzzly
```

Put TLS in front of it. The process speaks plain HTTP and does not pretend otherwise.

## Verify

```bash
cargo test                                   # 87 tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
mcp-gryzzly --list-tools                     # the surface, as registered
mcp-gryzzly --check                           # the configuration, against the live API
```

The suite includes the protocol itself: `tests/mcp_stdio.rs` spawns the real binary,
speaks JSON-RPC to its stdin and asserts on what comes back — the handshake, all 53
tools with their schemas, the resources, the prompts, an ambiguous name, a refused
delete, read-only mode. `tests/api_contract.rs` holds every quirk in the table above
against a fake Gryzzly, and `tests/mcp_http.rs` checks the HTTP transport and its
bearer guard.

```
src/
├── client.rs      RPC over HTTP: rate limiting, retries, the empty-list 404
├── config.rs      the environment, validated once, with the key redacted
├── resolve.rs     names to identifiers, and the refusal to guess
├── duration.rs    2h30, 90m, 1.5h, 2:30
├── dates.rs       today, monday, 2026-W38
├── model.rs       the Gryzzly objects, read leniently
├── tools/         one module per toolset, one function per tool
├── resources.rs   four URIs and two templates
├── prompts.rs     three workflows that ask before writing
└── server.rs      capabilities, instructions, and the glue
```

## Contributing

Issues and pull requests welcome. A change is ready when `cargo test`,
`cargo clippy --all-targets -- -D warnings` and `cargo fmt --all --check` pass, and
when a new tool carries a description, an annotation and an output schema — three
tests will tell you if it does not.

<details>
<summary><b>Repository topics</b></summary>

```bash
gh repo edit edouard-claude/mcp-gryzzly --add-topic \
mcp,model-context-protocol,mcp-server,gryzzly,time-tracking,timesheet,rust,rmcp,llm-tools,ai-agents,claude,claude-desktop,claude-code,anthropic,api-client,productivity,developer-tools,automation,stdio,streamable-http
```
</details>

## License

MIT — see [LICENSE](LICENSE). Not affiliated with Gryzzly.
