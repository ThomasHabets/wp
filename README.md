# wp — Wrap pipe

<https://github.com/ThomasHabets/wp>

This wraps the pipe by embedding metadata in the stdout and stdin streams, so
that the final step in a pipeline can tell the difference between EOF coming
from failure and EOF coming from actual EOF.

The main use case is if the last step in the process has a "commit" step that it
wants to execute on EOF, that we do NOT want to happen if the data generator
failed.

Examples:
* Streaming backups to a cloud storage uploader.
* Streaming database updated to the database's CLI.

## Usage

`wp` wraps either stdin (`-i`), stdout (`-o`), or both (`-io`), of its child
process.

### Example: backup script

```shell
wp -o tar cf - /some/path | wp -io xz -9 | wp -i upload-to-gcs
```

### Example: checksum a remote file

This ensures that we never see a checksum of a partial file, if the network
broke the SSH connection.

```shell
ssh shell.example.com "wp -o cat test.bin" | wp -i sha1sum -
```

## What happens if a child process exits with non-zero?

Then the downstream process will just get the EOF without the special `wp`
sauce, and the downstream `wp` will kill its child process instead of
"forwarding" that EOF.

That will make database transactions an cloud storage uploads be abandoned.

## Corner cases

The main invariant, that upstream processes in the pipe never cause downstream
to receive a clean EOF, should always be upheld. But there are some other corner
cases:

1. If a `wp -i` child process closes its stdin while data is still coming, that
   counts as failure and causes the child process to be killed, and anything
   downstream will not be committed.
2. If a `wp -o` stdout closes (e.g. if a downstream process failed), then the
   child process will also get a `SIGPIPE` if it continues writing. This is
   expected.
3. If that child process ignores the `SIGPIPE`, then nothing else will kill it.
   This is probably the best option, since we should assume that if it ignores
   the signal then it knows what it's doing.

## Build static

```
rustup target add x86_64-unknown-linux-musl
cargo build --release --target=x86_64-unknown-linux-musl
```

Build output will now be
`target/x86_64-unknown-linux-musl/release/wp`.
