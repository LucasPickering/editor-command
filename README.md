# editor-command

[![Test CI](https://github.com/github/docs/actions/workflows/test.yml/badge.svg)](https://github.com/LucasPickering/editor-command/actions)
[![crates.io](https://img.shields.io/crates/v/editor-command.svg)](https://crates.io/crates/editor-command)
[![docs.rs](https://img.shields.io/docsrs/editor-command)](https://docs.rs/editor-command)

Load a user's preferred file editing command from the `VISUAL` or `EDITOR` environment variables.

```rust
use editor_command::EditorCommand;
use std::process::Command;

std::env::set_var("VISUAL", "vim");
let mut command: Command = EditorCommand::edit_file("file.txt").unwrap();
command.spawn();
```
