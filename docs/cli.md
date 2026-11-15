# Command-line interface

Light Whisper provides a CLI for desktop control and local engine diagnostics.
It writes JSON to standard output. Diagnostics go to standard error.

## Open the installed CLI

Use PowerShell:

```powershell
$cli = Join-Path $env:LOCALAPPDATA '轻语 Whisper\light-whisper-cli.exe'
& $cli --help
& $cli doctor
```

The installed CLI does not need Python or Node.js.
For a source checkout, use `pnpm --silent cli` or `python scripts/light_whisper_cli.py`.
Put global options before the command.

## Select a target

| Target | Behavior | Requirements |
| --- | --- | --- |
| `app` | Connect to the current desktop instance. | A compatible desktop application must be in operation. |
| `engine` | Start and own a separate local engine process. | An installed engine or a prepared source environment and local model files. |

The default target is `app`.
The options `--engine`, `--python`, `--engine-exe` and `--data-dir` select `engine`.
Use `--app-exe` to select a desktop executable.
Incompatible target options produce an error before a process starts.

```powershell
& $cli --target app status
& $cli --target engine status
& $cli --engine confucius4-r2t2 status
```

Independent mode reads the engine selection from the desktop configuration.
An explicit `--engine` value takes priority.
Use a local engine in independent mode.
The supported local values are `qwen3-asr-0.6b` and `confucius4-r2t2`.

## Inspect the desktop

```powershell
& $cli status
& $cli stats
& $cli engine get
& $cli models
& $cli devices
```

`status` distinguishes the saved engine selection from observed runtime state.
It reports when the engine is busy. It does not stop a recording.
Its readiness fields describe the local engine process.
It does not test a cloud provider connection.
`stats` reads the current engine process. Its counters reset when that process restarts.
`models` checks local model files. `devices` lists input devices.
The device command does not open a microphone.

To change the local engine, run:

```powershell
& $cli engine set confucius4-r2t2
& $cli engine start
```

Engine selection saves the setting and stops the previous engine.
The desktop can load the selected model when it refreshes status.
Use `status` to inspect it.
Use `engine start` to load the selected engine when it is stopped.
Use `engine restart` to restart it.
These operations report a busy state if speech recognition owns the engine.

The desktop interface does not provide arbitrary application commands, credentials,
microphone activation or recording cancellation.
The desktop application does not stop when the CLI connection closes.

## Transcribe a file

Replace `sample.wav` with an existing audio file:

```powershell
& $cli transcribe sample.wav --language en --hot-word 'Light Whisper'
& $cli --engine confucius4-r2t2 transcribe sample.wav --context 'Medical dictation'
```

File transcription returns the result in JSON.
It does not paste text or create a desktop transcription-history entry.
The desktop file interface supports local engines and files up to 64 MiB.

Desktop request records have a 64 KiB limit, including JSON escapes and the line ending.
Invalid Unicode and oversized requests fail before dispatch.
A rejected session request does not close the session.

## Reuse one connection

Use `session` for several files or related diagnostic commands.
Send one UTF-8 JSON object per line.
Each request must have an integer `request_id` and an `action`.
Both targets support `status`, `stats`, `transcribe` and `exit`.
Desktop sessions also support `models`, `devices`, `engine_get`, `engine_set`,
`engine_start` and `engine_restart`.
For `engine_set`, include an `engine` field with a supported local engine name.

```powershell
@'
{"request_id":10,"action":"status"}
{"request_id":11,"action":"transcribe","audio":"sample.wav","language":"en","hot_words":["Light Whisper"]}
{"request_id":12,"action":"stats"}
{"request_id":13,"action":"exit"}
'@ | & $cli --target engine session
```

A session starts its child only when it receives a valid operation.
It reuses that child until exit, end of input or a terminal error.
Independent sessions use one engine process for several files.
For desktop sessions, the child is a connection broker for the running application.

Transcription requests accept `audio`, `language`, `context` and `hot_words`.
Optional language and context values must be strings.
Hot words must be an array of strings.
For a single transcription, repeat `--hot-word` for each term.
The response preserves the caller's request ID.

Invalid records and explicit operation failures allow the session to continue.
A startup failure, lost connection, response timeout or nonfinite number in a
matching engine response ends the session.
The CLI does not retry an operation with an unknown outcome.
An admitted desktop operation can finish after its client disconnects.

## Diagnose an independent engine

```powershell
$engineExe = Join-Path $env:APPDATA 'com.light-whisper.app\engine\engine.exe'
& $cli --engine-exe $engineExe --timeout 120 --threads 2 status
pnpm --silent cli --python .venv/Scripts/python.exe --engine confucius4-r2t2 status
```

Run the Python command from a prepared source checkout. The installed CLI uses
an engine executable; it does not include the Python engine source files.

Independent mode sets the native inference thread budget to two by default.
`--threads` changes this budget. It does not select CPU or GPU execution.
The bundled engine selects an available backend.
It also uses offline model access.
It does not download model files.
It uses the saved model directory, then `HF_HOME/hub`, then the default Hugging Face cache.
An explicit runtime path takes priority over automatic discovery.

By default, each independent run uses a temporary data directory.
The CLI removes that directory after its child exits.
Use `--data-dir` to retain engine logs:

```powershell
& $cli --data-dir .\cli-evidence status > .\status.json
```

This option does not change the desktop configuration directory.
Use `LIGHT_WHISPER_DATA_DIR` to select that directory for an isolated application instance.
Keep evidence files outside temporary runtime directories.
Transcription results can contain private speech.

## Read results

`doctor` reports paths, configuration and file presence. It does not load a model.
Its success flag means that it produced a report.
`model_load_checked: false` means that it did not test inference.
A model-file check also differs from a successful transcription.

| Exit code | Meaning |
| --- | --- |
| `0` | The command succeeded, or a session ended normally. |
| `1` | A runtime or operation failed. |
| `2` | Arguments, paths or configuration were invalid. |
| `130` | The user interrupted the CLI. |

Check every session response. A normally ended session can contain recoverable failures.
`--help` displays command help instead of JSON.

See [development](development.md) for source builds and verification.
