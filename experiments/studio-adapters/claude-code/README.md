# Claude Code adapter experiments

Two model-free probes, using an explicitly supplied local Claude Code executable:

- `probe.py`: isolated authentication status, strict text-plugin validation,
  invalid-manifest control, and session-only skill discovery.
- `run-mod.mjs`: prepare saved evidence with the shared presentation core, stage
  `native-mod/`, then run Claude Code's own offline native-mod tests.

Requires Linux, `/usr/bin/python3`, Node 24, and the shared core's workspace
dependencies. Native-mod commands were tested with Claude Code **2.1.318**.
There is no separate adapter build.

## Run from the repository root

Set `CLAUDE` to the installed executable's absolute path, `NODE` to Node, and
`EVIDENCE_DIR` to a directory containing the original `rent-available-bicycle.json`
and `reject-unavailable-bicycle.json` saved captures:

```sh
python3 experiments/studio-adapters/claude-code/probe.py --claude "$CLAUDE"
"$NODE" experiments/studio-adapters/claude-code/run-mod.mjs \
  --claude "$CLAUDE" \
  --evidence-dir "$EVIDENCE_DIR" \
  --out artifacts/studio-adapters/tools/claude-native-mod-new
```

`--out` must be a new directory with an existing parent. Omitting `--evidence-dir`
uses only synthetic fixtures. Each run retains the staged plugin, read-only
synthetic inputs, input/core hashes, command outputs, and `result.json` under
`--out`; disposable HOME/configuration/cache are removed even on CLI failure.
The output and its directories are `0700`, normal files `0600`, and immutable
synthetic inputs/payload-bearing snapshots `0400`. Staging includes payloads even
before UI inspection; it does not widen access to owner-only source files.
Delete that owned output directory to clean up the retained experiment.

Exit **0** requires every requested input to import, strict validation and all
three native tests to pass, original bytes to remain unchanged, and cleanup to
succeed. **Any explicit input failure returns nonzero**, even if native tests
pass on other inputs. `result.json` retains `importFailures` (including core
diagnostics), command results, cleanup status and `success`. Input acquisition
uses fatal UTF-8 decoding and SHA-256 of the actual bytes, before decoding.

The native runner requires exactly `claude plugin test <staged-plugin>`: options
before `plugin` are rejected, and `--bare` disables mods. It runs under a minimal
environment with owned HOME, Claude configuration, and XDG directories; other
CLI checks also use `--setting-sources ""`. No model session is started.
Each command owns a new process group. The Linux supervisor enforces 60 seconds,
then SIGTERM, a 250 ms grace period, and SIGKILL; reaping has a separate 2-second
bound. Cleanup reads only the single-threaded supervisor's kernel child list at
`/proc/self/task/<supervisor-pid>/children`. It signals and reaps direct/adopted
children, including `setsid()` descendants left after their leader exits; newly
orphaned children are handled on subsequent bounded sweeps. It does not scan
system processes or send signals to a possibly reused process-group ID.
Success requires both `groupCleaned` and `childrenCleaned`; remaining owned
children, including unreaped zombies, produce failure and `remainingChildren`
diagnostics. The original group is checked for absence, not used as proof that
all descendants are gone.
While a command is running, SIGINT/SIGTERM to the runner are forwarded to this
cleanup path.

## Regressions

These use the actual installed offline engine for adapter tests, plus disposable
signal-ignoring processes for the supervisor:

```sh
CLAUDE="$CLAUDE" \
ADAPTER_TEST_ARTIFACTS="$(mktemp -d artifacts/studio-adapters/tools/claude-native-checks-XXXXXXXX)" \
  "$NODE" --test experiments/studio-adapters/claude-code/runner.test.mjs
```

Tests cover invalid explicit captures and retained diagnostics, malformed UTF-8,
byte-identical copies, owner-only payload inputs under caller umask `022`, hard
timeout escalation, and cleanup of descendants after their leader exits. The
exact independently reviewed fork/`setsid()`/readiness-pipe reproduction has its
own independent subreaper cleanup owner. A separate failure-injection test checks
that an unreapable owned child fails even with a clean group and leader exit 0;
its illustrative PID is never signalled. Test artifacts remain in the private
directory named by `ADAPTER_TEST_ARTIFACTS`.

## Pane behavior and tested boundary

The staged plugin adds these commands:

- `/studio-pane`: open the saved-capture selector, operation/message list, and
  message-inspection selector.
- `/studio-selection`: return selected identity as command-result JSON, or `null`.
- `/studio-select <JSON>`: inspect an identity in the active capture; reject
  mismatched source, digest, operation, or message without changing the selection.

Identity is `{captureId, source, snapshotId, operationId, messageId}`: `captureId`
is SHA-256 of original bytes, `source` is the absolute path, and `snapshotId` comes
from the core's `createSelection(view, messageId)`. Canonical IDs are unchanged.
Capture-option keys encode `[source, captureId]`, so byte-identical copies remain
distinct. Callbacks retain the snapshot that drew them; they never rebind stale
IDs to the current view. Switching captures clears selection; closing/reopening
keeps it within the module lifetime. Regenerating the staged data opens new
snapshots and requires reselection; reloading the module clears its selection.

The shared [`tracer-presentation`](../../../packages/tracer-presentation/README.md)
core performs all evidence reading, selection resolution, and plain-text
rendering in Node. The generated plugin-local snapshot contains only derived
text and canonical identity mappings, frozen when the mod loads. This staging
respects the mods API's plugin-local import restriction; it adds no second
evidence interpreter. Payloads appear only in the explicit inspection text.

Three synthetic resources reuse the core's labeled test fixture and deliberately
repeat operation/message IDs under different saved-file digests and under an
identical digest at two different paths. Tests dispatch
native `ui.select` and `command.run` events, verify exact returned identities and
inspection text, reject malformed/stale selections, and remount the pane.
Console diagnostics contain the actual command results; the runner places them
in its stderr artifact. Original input and generated snapshot hashes are checked
afterward.

This exercises **Claude Code's native mod test engine and element-tree validation**
with `terminal` and `desktop` surface parameters. Session startup, command
registration, and pane placement use documented test stubs. It does not verify
screen painting, keyboard ergonomics, model-context delivery, a signed-in
terminal/Desktop Code session, or MCP Apps rendering.

Official API sources: [create](https://code.claude.com/docs/en/plugins/mods/create.md),
[interface](https://code.claude.com/docs/en/plugins/mods/interface.md), and
[offline tests](https://code.claude.com/docs/en/plugins/mods/test.md).
