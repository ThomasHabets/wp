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

```
wp -o tar cf - /some/path | wp -io xz -9 | wp -i upload-to-gcs
```

## What happens if a child process exits with non-zero?

Then the downstream process will just get the EOF without the special `wp`
sauce, and the downstream `wp` will kill its child process instead of
"forwarding" that EOF.

That will make database transactions an cloud storage uploads be abandoned.

## Build static

```
rustup target add x86_64-unknown-linux-musl
cargo build --release --target=x86_64-unknown-linux-musl
```

Build output will now be
`target/x86_64-unknown-linux-musl/release/wp`.
