//! Get an executable [Command] to open a particular file in the user's
//! configured editor.
//!
//! ## Features
//!
//! - Load editor command from the `VISUAL` or `EDITOR` environment variables
//! - Specify high-priority override and low-priority default commands to use
//! - Pass one or more paths to be opened by the editor
//! - Flexible builder pattern
//!
//! ## Examples
//!
//! The simplest usage looks like this:
//!
//! ```
//! # // Hide this part because it doesn't provide any value to the user
//! # let _guard = env_lock::lock_env([
//! #     ("VISUAL", None::<&str>),
//! #     ("EDITOR", None),
//! # ]);
//! use editor_command::EditorCommand;
//! use std::process::Command;
//!
//! std::env::set_var("VISUAL", "vim");
//! let command: Command = EditorCommand::edit_file("file.txt").unwrap();
//! assert_eq!(command.get_program(), "vim");
//! ```
//!
//! Here's an example of using the builder pattern to define a fallback command:
//!
//! ```
//! # // Hide this part because it doesn't provide any value to the user
//! # let _guard = env_lock::lock_env([
//! #     ("VISUAL", None::<&str>),
//! #     ("EDITOR", None),
//! # ]);
//! use editor_command::EditorCommand;
//! use std::process::Command;
//!
//! let command: Command = EditorCommand::new()
//!     .environment()
//!     // If both VISUAL and EDITOR are undefined, we'll fall back to this
//!     .source(Some("vi"))
//!     .build()
//!     .unwrap();
//! assert_eq!(command.get_program(), "vi");
//! ```
//!
//! ## Lifetimes
//!
//! [EditorCommand] accepts a lifetime parameter, which is bound to the string
//! data it contains (both command strings and paths). This is to prevent
//! unnecessary cloning when building commands/paths from `&str`s. If you need
//! the instance of [EditorCommand] to be `'static`, e.g. so it can be returned
//! from a function, you can simply use `EditorCommand<'static>`. Internally,
//! all strings are stored as [Cow]s, so clones will be made as necessary.
//!
//! ```rust
//! use editor_command::EditorCommand;
//!
//! /// This is a contrived example of returning a command with owned data
//! fn get_editor_command<'a>(command: &'a str) -> EditorCommand<'static> {
//!     // The lifetime bounds enforce the .to_owned() call
//!     EditorCommand::new().source(Some(command.to_owned()))
//! }
//!
//! let command = get_editor_command("vim").build().unwrap();
//! assert_eq!(command.get_program(), "vim");
//! ```
//!
//! ## Resources
//!
//! For more information on the `VISUAL` and `EDITOR` environment variables,
//! [check out this thread](https://unix.stackexchange.com/questions/4859/visual-vs-editor-what-s-the-difference).

use shellish_parse::ParseOptions;
use std::{
    borrow::Cow,
    env,
    error::Error,
    fmt::{self, Display},
    path::Path,
    process::Command,
};

/// A builder for a [Command] that will open the user's configured editor. For
/// simple cases you probably can just use [EditorCommand::edit_file]. See
/// [module-level documentation](super) for more details and examples.
#[derive(Clone, Debug, Default)]
pub struct EditorCommand<'a> {
    /// Command to parse. This will be populated the first time we're given a
    /// source with a value. After that, it remains unchanged.
    command: Option<Cow<'a, str>>,
    /// Path(s) to pass as the final argument(s) to the command
    paths: Vec<Cow<'a, Path>>,
}

impl<'a> EditorCommand<'a> {
    /// Create a new editor command with no sources. You probably want to call
    /// [environment](Self::environment) on the returned value.
    pub fn new() -> Self {
        Self::default()
    }

    /// Shorthand for opening a file with the command set in `VISUAL`/`EDITOR`.
    ///
    /// ```ignore
    /// EditorCommand::edit_file("file.yml")
    /// ```
    ///
    /// is equivalent to:
    ///
    /// ```ignore
    /// EditorCommand::new().environment().path(path).build()
    /// ```
    pub fn edit_file(
        path: impl AsRef<Path>,
    ) -> Result<Command, EditorCommandError> {
        Self::new().environment().path(path.as_ref()).build()
    }

    /// Add a static string as a source for the command. TODO more
    pub fn source(mut self, source: Option<impl Into<Cow<'a, str>>>) -> Self {
        self.command = self.command.or(source.map(Into::into));
        self
    }

    /// Add the `VISUAL` and `EDITOR` environment variables, in that order. The
    /// variables will be evaluated **immediately**, *not* during
    /// [build](Self::build).
    pub fn environment(mut self) -> Self {
        // Populate command if it isn't already
        self.command = self
            .command
            .or_else(|| env::var("VISUAL").ok().map(Cow::from))
            .or_else(|| env::var("EDITOR").ok().map(Cow::from));
        self
    }

    /// Define the path to be passed as the final argument.
    ///
    /// ## Multiple Calls
    ///
    /// Subsequent calls to this on the same instance will append to the list
    /// of paths. The paths will all be included in the final command, in the
    /// order this method was called.
    pub fn path(mut self, path: impl Into<Cow<'a, Path>>) -> Self {
        // TODO accept &str, &Path, and PathBuf as args
        self.paths.push(path.into());
        self
    }

    /// Search all configured sources (in their order of definition), and parse
    /// the first one that's populated as a shell command. Then use that to
    /// build an executable [Command].
    ///
    /// ## Command Syntax
    ///
    /// The loaded command string will be parsed as most common shells do. For
    /// specifics about parsing, see the crate [shellish_parse].
    pub fn build(self) -> Result<Command, EditorCommandError> {
        // Find the first source that has a value. We *don't* validate that the
        // command is non-empty or parses. If something has a value, it's better
        // to use it and give the user an error if it's invalid, than to
        // silently skip past it.
        let command_str = self.command.ok_or(EditorCommandError::NoCommand)?;

        // Parse it as a shell command
        let mut parsed =
            shellish_parse::parse(&command_str, ParseOptions::default())
                .map_err(EditorCommandError::ParseError)?;

        // First token is the program name, rest are arguments
        let mut tokens = parsed.drain(..);
        let program = tokens.next().ok_or(EditorCommandError::EmptyCommand)?;
        let args = tokens;

        let mut command = Command::new(program);
        command
            .args(args)
            .args(self.paths.iter().map(|path| path.as_os_str()));
        Ok(command)
    }
}

/// Any error that can occur while loading the editor command.
#[derive(Debug)]
pub enum EditorCommandError {
    /// Couldn't find an editor command anywhere
    NoCommand,

    /// The editor command was found, but it's just an empty/whitespace string
    EmptyCommand,

    /// Editor command couldn't be parsed in a shell-like format
    ParseError(shellish_parse::ParseError),
}

impl Display for EditorCommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditorCommandError::NoCommand => write!(
                f,
                "Edit command not defined in any of the listed sources"
            ),
            EditorCommandError::EmptyCommand => {
                write!(f, "Editor command is empty")
            }
            EditorCommandError::ParseError(source) => {
                write!(f, "Invalid editor command: {source}")
            }
        }
    }
}

impl Error for EditorCommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            EditorCommandError::NoCommand
            | EditorCommandError::EmptyCommand => None,
            EditorCommandError::ParseError(source) => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{ffi::OsStr, path::PathBuf};

    /// Test loading from a static source that overrides the environment
    #[test]
    fn source_priority() {
        let builder = {
            let _guard = env_lock::lock_env([
                ("VISUAL", Some("visual")),
                ("EDITOR", Some("editor")),
            ]);
            EditorCommand::new()
                .source(None::<&str>)
                .source(Some("priority"))
                .environment()
                .source(Some("default"))
        };
        assert_cmd(builder, "priority", &[]);
    }

    /// Test loading from the `VISUAL` env var
    #[test]
    fn source_visual() {
        let builder = {
            let _guard = env_lock::lock_env([
                ("VISUAL", Some("visual")),
                ("EDITOR", Some("editor")),
            ]);
            EditorCommand::new().environment().source(Some("default"))
        };
        assert_cmd(builder, "visual", &[]);
    }

    /// Test loading from the `EDITOR` env var
    #[test]
    fn source_editor() {
        let builder = {
            let _guard = env_lock::lock_env([
                ("VISUAL", None),
                ("EDITOR", Some("editor")),
            ]);
            EditorCommand::new().environment().source(Some("default"))
        };
        assert_cmd(builder, "editor", &[]);
    }

    /// Test loading from a fallback value, with lower precedence than the env
    #[test]
    fn source_default() {
        let builder = {
            let _guard = env_lock::lock_env([
                ("VISUAL", None::<&str>),
                ("EDITOR", None),
            ]);
            EditorCommand::new().environment().source(Some("default"))
        };
        assert_cmd(builder, "default", &[]);
    }

    /// Test included paths as extra arguments
    #[test]
    fn paths() {
        let builder = EditorCommand::new()
            .source(Some("ed"))
            // All of these types should be accepted, for ergonomics
            .path("path1")
            .path(Path::new("path2"))
            .path(PathBuf::from("path3".to_owned()));
        assert_cmd(builder, "ed", &["path1", "path2"]);
    }

    /// Test simple command parsing logic. We'll defer edge cases to
    /// shellish_parse
    #[test]
    fn parsing() {
        let builder = EditorCommand::new()
            .source(Some("ned '--single \" quotes' \"--double ' quotes\""));
        assert_cmd(
            builder,
            "ned",
            &["--single \" quotes", "--double ' quotes"],
        );
    }

    /// Test when all options are undefined
    #[test]
    fn error_no_command() {
        let _guard = env_lock::lock_env([
            ("VISUAL", None::<&str>),
            ("EDITOR", None::<&str>),
        ]);
        assert_err(
            EditorCommand::new().environment().source(None::<&str>),
            "Edit command not defined in any of the listed sources",
        );
    }

    /// Test when the command exists but is the empty string
    #[test]
    fn error_empty_command() {
        assert_err(
            EditorCommand::new().source(Some("")),
            "Editor command is empty",
        );
    }

    /// Test when a value can't be parsed as a command string
    #[test]
    fn error_invalid_command() {
        assert_err(
            EditorCommand::new().source(Some("'unclosed quote")),
            "Invalid editor command: dangling string",
        );
    }

    /// Assert that the builder creates the expected command
    fn assert_cmd(
        builder: EditorCommand,
        expected_program: &str,
        expected_args: &[&str],
    ) {
        let command = builder.build().unwrap();
        assert_eq!(command.get_program(), expected_program);
        assert_eq!(
            command
                .get_args()
                .filter_map(OsStr::to_str)
                .collect::<Vec<_>>(),
            expected_args
        );
    }

    /// Assert that the builder fails to build with the given error message
    fn assert_err(builder: EditorCommand, expected_error: &str) {
        let error = builder.build().unwrap_err();
        assert_eq!(error.to_string(), expected_error);
    }
}
