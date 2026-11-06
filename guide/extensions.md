# Writing a Glint extension

An extension is a program in its own folder. Glint starts it the first time its trigger word is typed and talks to it over stdin and stdout, one JSON message per line. Any language works.

## Security: read this first

**An extension is a normal program running with your user rights.** It can read your files and use the network whatever its manifest says. Glint cannot prevent that, so nothing runs until you enable it in Settings, and you should only enable extensions you trust.

What Glint does enforce:

- An extension only receives the text typed after its own trigger word, never your other searches.
- It can only ask Glint to copy text or open links if its manifest declares `clipboard` or `open_url`. Links must be `http` or `https`.
- One that hangs is killed: 800 ms to answer a search, 5 s to run an action. Crashes and bad output cannot affect Glint. After 3 failures in a row it stays stopped until you press Reload in Settings.

Other permission names, such as `network` or `filesystem`, are shown to you as the author's statement of what it does. They are not enforced.

## Quick start

Copy one of the examples into `%APPDATA%\Glint\extensions`, then open Glint Settings, press Reload, tick Enabled and Save:

| Example | Language | Trigger | Needs |
|---|---|---|---|
| [`extensions/uuid-python`](../extensions/uuid-python) | Python | `uuid` | Python 3 on PATH |
| [`extensions/timestamp-typescript`](../extensions/timestamp-typescript) | TypeScript | `ts` | Node 23.6+ on PATH |
| [`extensions/base64-rust`](../extensions/base64-rust) | Rust | `b64` | `cargo build --release` first |

While developing, press Reload in Settings after each change. It stops running extensions and re-reads manifests.

## manifest.json

```json
{
  "id": "my-extension",
  "name": "My extension",
  "version": "1.0.0",
  "trigger": "mine",
  "command": ["python", "-u", "main.py"],
  "permissions": ["clipboard", "open_url"]
}
```

- `id`: lower-case letters, digits and `-`.
- `trigger`: one word. Typing `mine some text` sends `some text` to the extension.
- `command`: the program and its arguments, run in the extension's folder. A program path containing a slash is relative to that folder; otherwise it is looked up on PATH.

## Protocol

Line-delimited JSON-RPC 2.0. Flush after each line. Reply with the same `id`.

### query

Sent as the user types. Answer quickly: results that take over 800 ms stop the extension.

```json
{"jsonrpc":"2.0","id":1,"method":"query","params":{"text":"some text"}}
```

```json
{"jsonrpc":"2.0","id":1,"result":{"items":[{"id":"a","title":"Shown in bold","subtitle":"Shown below"}]}}
```

At most 50 items are used. `subtitle` is optional.

### run

Sent when the user presses Enter on one of your items.

```json
{"jsonrpc":"2.0","id":2,"method":"run","params":{"id":"a"}}
```

```json
{"jsonrpc":"2.0","id":2,"result":{"effects":[{"type":"copy","text":"hello"},{"type":"open_url","url":"https://example.com"}]}}
```

Effects are `copy`, which needs `clipboard`, and `open_url`, which needs `open_url`. `effects` may be empty. If any effect is not allowed, none of them run.

### Errors and stopping

Report a failure with `{"jsonrpc":"2.0","id":2,"error":{"code":-1,"message":"why"}}`. Your program must exit when stdin closes, because that is how Glint stops it on a clean shutdown. Anything written to stderr is ignored.

## Sharing

Put the folder at the root of a Git repository. Others can install it from Settings with its `https://` URL. It stays disabled until they enable it. Extensions in compiled languages need building after install, so commit a release binary or document the build step.
