//! Get an executable [Command] to open a particular file in the user's
//! configured editor.
//!
//! ## Features
//!
//! - Load editor command from the `VISUAL` or `EDITOR` environment variables
//! - Specify high-priority override and low-priority default commands to use
//! - Open files to a particular line/column
//! - Flexible builder pattern
//!
//! ## Examples
//!
//! `editor-command` uses a two-stage abstraction:
//!
//! - Build an [Editor] (optionally using an [EditorBuilder]), which represents
//!   a user's desired editor
//! - Use [Editor::open] to build a [Command] that will open a particular file
//!
//! ### Simplest Usage
//!
//! ```
//! # let _guard = env_lock::lock_env([
//! #     ("VISUAL", None::<&str>),
//! #     ("EDITOR", None),
//! # ]);
//! use editor_command::Editor;
//! use std::process::Command;
//!
//! std::env::set_var("VISUAL", "vim");
//! // Building an editor is fallible because the user's configured command may
//! // be invalid (e.g. it could have unclosed quotes)
//! let editor = Editor::new().unwrap();
//! // Once we have an editor, building a Command is infallible
//! let command: Command = editor.open("file.txt");
//!
//! assert_eq!(command.get_program(), "vim");
//! assert_eq!(command.get_args().collect::<Vec<_>>(), &["file.txt"]);
//!
//! // You can spawn the editor with:
//! // command.status().unwrap();
//! ```
//!
//! ### Open to Line/Column
//!
//! You can open a file to particular line/column using [Editor::open_at]:
//!
//! ```
//! # let _guard = env_lock::lock_env([
//! #     ("VISUAL", None::<&str>),
//! #     ("EDITOR", None),
//! # ]);
//! use editor_command::Editor;
//! use std::process::Command;
//!
//! std::env::set_var("VISUAL", "vim");
//! let editor = Editor::new().unwrap();
//! let command: Command = editor.open_at("file.txt", 10, 5);
//!
//! assert_eq!(command.get_program(), "vim");
//! assert_eq!(
//!     command.get_args().collect::<Vec<_>>(),
//!     &["file.txt", "+call cursor(10, 5)"],
//! );
//! ```
//!
//! See [Editor::open_at] for info on how it supports line/column for various
//! editors, and how to support it for arbitrary user-provided commands.
//!
//! ### Overrides and Fallbacks
//!
//! Here's an example of using [EditorBuilder] to provide both an override
//! and a fallback command:
//!
//! ```
//! # let _guard = env_lock::lock_env([
//! #     ("VISUAL", None::<&str>),
//! #     ("EDITOR", None),
//! # ]);
//! use editor_command::EditorBuilder;
//! use std::process::Command;
//!
//! std::env::set_var("VISUAL", "vim"); // This gets overridden
//! let editor = EditorBuilder::new()
//!     // In this case, the override is always populated so it will always win.
//!     // In reality it would be an optional user-provided field.
//!     .string(Some("code --wait"))
//!     .environment()
//!     // If both VISUAL and EDITOR are undefined, we'll fall back to this
//!     .string(Some("vi"))
//!     .build()
//!     .unwrap();
//! let command = editor.open("file.txt");
//!
//! assert_eq!(command.get_program(), "code");
//! assert_eq!(command.get_args().collect::<Vec<_>>(), &["--wait", "file.txt"]);
//! ```
//!
//! This pattern is useful for apps that have a way to configure an app-specific
//! editor. For example, [git has the `core.editor` config field](https://git-scm.com/book/en/v2/Customizing-Git-Git-Configuration).
//!
//! ### Tokio
//!
//! [Editor] returns a `std` [Command], which will execute synchronously.
//! If you want to run your editor subprocess asynchronously via
//! [tokio](https://docs.rs/tokio/latest/tokio/), use the
//! `From<std::process::Command>` impl on `tokio::process::Command`. For
//! example:
//!
//! ```ignore
//! let editor = Editor::new().unwrap();
//! let command: tokio::process::Command = editor.open("file.yaml").into();
//! ```
//!
//! ## Syntax
//!
//! The syntax of the command is meant to resemble command syntax for common
//! shells. The first word is the program name, and subsequent tokens (separated
//! by spaces) are arguments to that program. Single and double quotes can be
//! used to join multiple tokens together into a single argument.
//!
//! Command parsing is handled by the crate [shell-words](shell_words). Refer to
//! those docs for exact details on the syntax.
//!
//! ## Resources
//!
//! For more information on the `VISUAL` and `EDITOR` environment variables,
//! [check out this thread](https://unix.stackexchange.com/questions/4859/visual-vs-editor-what-s-the-difference).

use std::{
    borrow::Cow,
    env,
    error::Error,
    fmt::{self, Display},
    path::Path,
    process::Command,
    str::FromStr,
};

/// An editor is a builder for [Command]s. An `Editor` instance represent's a
/// user's desired editor, and can be used repeatedly to open files.
#[derive(Clone, Debug)]
pub struct Editor {
    /// Binary to invoke
    program: String,
    known: Option<KnownEditor>,
    /// Arguments to pass to the binary
    arguments: Vec<String>,
}

impl Editor {
    /// Create an editor from the user's `$VISUAL` or `$EDITOR` environment
    /// variables. This is the easiest way to create an editor, but provides no
    /// flexibility. See the [crate-level
    /// documentation](crate#overrides-and-fallbacks) for an example of how
    /// to use [EditorBuilder] to customize overrides and fallbacks.
    ///
    /// ```no_run
    /// # use editor_command::{Editor, EditorBuilder};
    /// Editor::new().unwrap();
    /// // is equivalent to
    /// EditorBuilder::new().environment().build().unwrap();
    /// ```
    ///
    /// ### Errors
    ///
    /// Returns an error if:
    /// - Neither `$VISUAL` nor `$EDITOR` is defined
    /// - The command fails to parse (e.g. dangling quote)
    pub fn new() -> Result<Self, EditorBuilderError> {
        EditorBuilder::new().environment().build()
    }

    /// Build a command that will open a file
    pub fn open(&self, path: impl AsRef<Path>) -> Command {
        let mut command = Command::new(&self.program);
        command.args(&self.arguments).arg(path.as_ref());
        command
    }

    /// Build a command that will open a file to a particular line and column.
    ///
    /// Most editors accept the format `path:line:column`, so that's used by
    /// default. This method supports some specific editors that don't follow
    /// that convention. It will automatically detect these editors based on the
    /// invoked command and pass the line/column accordingly:
    ///
    /// - `emacs`
    /// - `vi`/`vim`/`nvim`
    /// - `nano` (column not supported)
    ///
    /// If you want support for another editor that's not listed here, please
    /// [open an issue on GitHub](https://github.com/LucasPickering/editor-command/issues/new/choose).
    pub fn open_at(
        &self,
        path: impl AsRef<Path>,
        line: u32,
        column: u32,
    ) -> Command {
        let path = path.as_ref();
        let mut command = Command::new(&self.program);
        command.args(&self.arguments);

        if let Some(known) = self.known {
            // This editor requires special logic to open to a line/col
            known.open_at(&mut command, path, line, column);
        } else {
            // This is a common format, so hope the editor supports it
            command
                .arg(format!("{path}:{line}:{column}", path = path.display()));
        }

        command
    }
}

/// A builder for customizing an [Editor]. In simple cases you can just use
/// [Editor::new] and don't have to interact with this struct. See [crate-level
/// documentation](crate#overrides-and-fallbacks) for more details and examples.
///
/// ## Example
///
/// The builder works by calling one or more "source" methods. Each source may
/// (or may not) provide an editor command. The first source that provides a
/// command will be used, and subsequent sources will be ignored. For example,
/// here's a builder that uses 3 sources:
///
/// - User's configured editor
/// - Environment variables
/// - Static fallback
///
/// ```
/// # let _guard = env_lock::lock_env([
/// #     ("VISUAL", None::<&str>),
/// #     ("EDITOR", None),
/// # ]);
/// use editor_command::EditorBuilder;
/// use std::process::Command;
///
/// std::env::set_var("VISUAL", "vim"); // This gets overridden
/// let editor = EditorBuilder::new()
///     .string(configured_editor())
///     .environment()
///     // If both VISUAL and EDITOR are undefined, we'll fall back to this
///     .string(Some("vi"))
///     .build()
///     .unwrap();
/// let command = editor.open("file.txt");
///
/// assert_eq!(command.get_program(), "code");
/// assert_eq!(command.get_args().collect::<Vec<_>>(), &["--wait", "file.txt"]);
///
/// fn configured_editor() -> Option<String> {
///     // In reality this would load from a config file or similar
///     Some("code --wait".into())
/// }
/// ```
///
/// ## Lifetimes
///
/// [EditorBuilder] accepts a lifetime parameter, which is bound to the string
/// data it contains (both command strings and paths). This is to prevent
/// unnecessary cloning when building editors from `&str`s. If you need
/// the instance of [EditorBuilder] to be `'static`, e.g. so it can be returned
/// from a function, you can simply use `EditorBuilder<'static>`. Internally,
/// all strings are stored as [Cow]s, so clones will be made as necessary. Once
/// the builder is converted into an [Editor], all strings will be cloned.
///
/// ```rust
/// use editor_command::EditorBuilder;
///
/// /// This is a contrived example of returning a command with owned data
/// fn get_editor_builder<'a>(command: &'a str) -> EditorBuilder<'static> {
///     // The lifetime bounds enforce the .to_owned() call
///     EditorBuilder::new().string(Some(command.to_owned()))
/// }
///
/// let editor = get_editor_builder("vim").build().unwrap();
/// assert_eq!(editor.open("file").get_program(), "vim");
/// ```
#[derive(Clone, Debug, Default)]
pub struct EditorBuilder<'a> {
    /// Command to parse. This will be populated the first time we're given a
    /// source with a value. After that, it remains unchanged.
    command: Option<Cow<'a, str>>,
}

impl<'a> EditorBuilder<'a> {
    /// Create a new editor command with no sources. You probably want to call
    /// [environment](Self::environment) on the returned value.
    pub fn new() -> Self {
        Self::default()
    }

    /// Load the editor command from a string. This is useful for static
    /// defaults or external sources such as a configuration file. This accepts
    /// an `Option` so you can easily build a chain of sources that may or may
    /// not be defined.
    pub fn string(mut self, source: Option<impl Into<Cow<'a, str>>>) -> Self {
        self.command = self.command.or(source.map(Into::into));
        self
    }

    /// Load the editor command from the `VISUAL` and `EDITOR` environment
    /// variables, in that order. The variables will be evaluated immediately,
    /// *not* during [build](Self::build).
    pub fn environment(mut self) -> Self {
        // Populate command if it isn't already
        self.command = self
            .command
            .or_else(|| env::var("VISUAL").ok().map(Cow::from))
            .or_else(|| env::var("EDITOR").ok().map(Cow::from));
        self
    }

    /// Search all configured sources (in their order of definition), and parse
    /// the first one that's populated as a shell command. Then use that to
    /// build an executable [Command].
    pub fn build(self) -> Result<Editor, EditorBuilderError> {
        // Find the first source that has a value. We *don't* validate that the
        // command is non-empty or parses. If something has a value, it's better
        // to use it and give the user an error if it's invalid, than to
        // silently skip past it.
        let command_str = self.command.ok_or(EditorBuilderError::NoCommand)?;

        // Parse it as a shell command
        let mut parsed = shell_words::split(&command_str)
            .map_err(EditorBuilderError::ParseError)?;

        // First token is the program name, rest are arguments
        let mut tokens = parsed.drain(..);
        let program = tokens.next().ok_or(EditorBuilderError::EmptyCommand)?;
        let arguments = tokens.collect();
        // Check the program name to see if we recognize this editor
        let known = program.parse().ok();

        Ok(Editor {
            program,
            known,
            arguments,
        })
    }
}

/// Any error that can occur while loading the editor command.
#[derive(Debug)]
pub enum EditorBuilderError {
    /// Couldn't find an editor command anywhere
    NoCommand,

    /// The editor command was found, but it's just an empty/whitespace string
    EmptyCommand,

    /// Editor command couldn't be parsed in a shell-like format
    ParseError(shell_words::ParseError),
}

impl Display for EditorBuilderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditorBuilderError::NoCommand => write!(
                f,
                "Edit command not defined in any of the listed sources"
            ),
            EditorBuilderError::EmptyCommand => {
                write!(f, "Editor command is empty")
            }
            EditorBuilderError::ParseError(source) => {
                write!(f, "Invalid editor command: {source}")
            }
        }
    }
}

impl Error for EditorBuilderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            EditorBuilderError::NoCommand
            | EditorBuilderError::EmptyCommand => None,
            EditorBuilderError::ParseError(source) => Some(source),
        }
    }
}

/// A known editor that requires special logic to open to a line/column. Most
/// editors support the common `path:line:column` format and don't need to be
/// specified here.
#[derive(Copy, Clone, Debug)]
enum KnownEditor {
    Emacs,
    Nano,
    /// Also includes Vim and Neovim
    Vi,
    // If you add a variant here, make sure to update the docs on open_at
}

impl KnownEditor {
    // It'd be nice to use strum for this but I don't want it in the dep tree
    /// All variants of the enum
    const ALL: &'static [Self] = &[Self::Emacs, Self::Nano, Self::Vi];

    /// Add arguments to the given command to open a file at a particular
    /// line+column
    fn open_at(
        &self,
        command: &mut Command,
        path: &Path,
        line: u32,
        column: u32,
    ) {
        match self {
            KnownEditor::Emacs => {
                // Offset has to go first
                command.arg(format!("+{line}:{column}")).arg(path);
            }
            // From my 6 seconds of research, nano doesn't support column
            KnownEditor::Nano => {
                // Offset has to go first
                command.arg(format!("+{line}")).arg(path);
            }
            KnownEditor::Vi => {
                command
                    .arg(path)
                    .arg(format!("+call cursor({line}, {column})"));
            }
        }
    }

    fn programs(&self) -> &'static [&'static str] {
        match self {
            Self::Emacs => &["emacs"],
            Self::Nano => &["nano"],
            Self::Vi => &["vi", "vim", "nvim"],
        }
    }
}

impl FromStr for KnownEditor {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .iter()
            // Intentionally do a case-sensitive match, because binary names
            // are case-sensitive on some systems
            .find(|known| known.programs().contains(&s))
            .copied()
            .ok_or(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::path::PathBuf;

    /// Test loading from a static source that overrides the environment
    #[test]
    fn source_priority() {
        let editor = {
            let _guard = env_lock::lock_env([
                ("VISUAL", Some("visual")),
                ("EDITOR", Some("editor")),
            ]);
            EditorBuilder::new()
                .string(None::<&str>)
                .string(Some("priority"))
                .environment()
                .string(Some("default"))
                .build()
                .unwrap()
        };
        assert_cmd(editor.open("file"), "priority", &["file"]);
    }

    /// Test loading from the `VISUAL` env var
    #[test]
    fn source_visual() {
        let editor = {
            let _guard = env_lock::lock_env([
                ("VISUAL", Some("visual")),
                ("EDITOR", Some("editor")),
            ]);
            EditorBuilder::new()
                .environment()
                .string(Some("default"))
                .build()
                .unwrap()
        };
        assert_cmd(editor.open("file"), "visual", &["file"]);
    }

    /// Test loading from the `EDITOR` env var
    #[test]
    fn source_editor() {
        let editor = {
            let _guard = env_lock::lock_env([
                ("VISUAL", None),
                ("EDITOR", Some("editor")),
            ]);
            EditorBuilder::new()
                .environment()
                .string(Some("default"))
                .build()
                .unwrap()
        };
        assert_cmd(editor.open("file"), "editor", &["file"]);
    }

    /// Test loading from a fallback value, with lower precedence than the env
    #[test]
    fn source_default() {
        let editor = {
            let _guard = env_lock::lock_env([
                ("VISUAL", None::<&str>),
                ("EDITOR", None),
            ]);
            EditorBuilder::new()
                .environment()
                .string(Some("default"))
                .build()
                .unwrap()
        };
        assert_cmd(editor.open("file"), "default", &["file"]);
    }

    /// Test `open()` for known and unknown editors
    #[rstest]
    #[case::emacs("emacs", "emacs", &["file"])]
    #[case::nano("nano", "nano", &["file"])]
    #[case::vi("vi", "vi", &["file"])]
    #[case::vi_with_args("vi -b", "vi", &["-b", "file"])]
    #[case::vim("vim", "vim", &["file"])]
    #[case::neovim("nvim", "nvim", &["file"])]
    #[case::unknown("unknown --arg", "unknown", &["--arg", "file"])]
    fn open(
        #[case] command: &str,
        #[case] expected_program: &str,
        #[case] expected_args: &[&str],
    ) {
        let editor =
            EditorBuilder::new().string(Some(command)).build().unwrap();
        assert_cmd(editor.open("file"), expected_program, expected_args);
    }

    /// Test `open_at()` for known and unknown editors
    #[rstest]
    #[case::emacs("emacs", "emacs", &["+2:3", "file"])]
    // Nano doesn't support column
    #[case::nano("nano", "nano", &["+2", "file"])]
    #[case::vi("vi", "vi", &["file", "+call cursor(2, 3)"])]
    #[case::vi_with_args("vi -b", "vi", &["-b", "file", "+call cursor(2, 3)"])]
    #[case::vim("vim", "vim", &["file", "+call cursor(2, 3)"])]
    #[case::neovim("nvim", "nvim", &["file", "+call cursor(2, 3)"])]
    // Default to path:line:column
    #[case::unknown("unknown --arg", "unknown", &["--arg", "file:2:3"])]
    fn open_at(
        #[case] command: &str,
        #[case] expected_program: &str,
        #[case] expected_args: &[&str],
    ) {
        let editor =
            EditorBuilder::new().string(Some(command)).build().unwrap();
        assert_cmd(
            editor.open_at("file", 2, 3),
            expected_program,
            expected_args,
        );
    }

    /// Test included paths as extra arguments
    #[test]
    fn paths() {
        let editor = EditorBuilder::new().string(Some("ed")).build().unwrap();
        // All of these types should be accepted, for ergonomics
        assert_cmd(editor.open("str"), "ed", &["str"]);
        assert_cmd(editor.open(Path::new("path")), "ed", &["path"]);
        assert_cmd(editor.open(PathBuf::from("pathbuf")), "ed", &["pathbuf"]);
    }

    /// Test simple command parsing logic. We'll defer edge cases to shell-words
    #[test]
    fn parsing() {
        let editor = EditorBuilder::new()
            .string(Some("ned '--single \" quotes' \"--double ' quotes\""))
            .build()
            .unwrap();
        assert_cmd(
            editor.open("file"),
            "ned",
            &["--single \" quotes", "--double ' quotes", "file"],
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
            EditorBuilder::new().environment().string(None::<&str>),
            "Edit command not defined in any of the listed sources",
        );
    }

    /// Test when the command exists but is the empty string
    #[test]
    fn error_empty_command() {
        assert_err(
            EditorBuilder::new().string(Some("")),
            "Editor command is empty",
        );
    }

    /// Test when a value can't be parsed as a command string
    #[test]
    fn error_invalid_command() {
        assert_err(
            EditorBuilder::new().string(Some("'unclosed quote")),
            "Invalid editor command: missing closing quote",
        );
    }

    /// Assert that the editor creates the expected command
    #[track_caller]
    fn assert_cmd(
        command: Command,
        expected_program: &str,
        expected_args: &[&str],
    ) {
        assert_eq!(command.get_program(), expected_program);
        assert_eq!(command.get_args().collect::<Vec<_>>(), expected_args);
    }

    /// Assert that the builder fails to build with the given error message
    #[track_caller]
    fn assert_err(builder: EditorBuilder, expected_error: &str) {
        let error = builder.build().unwrap_err();
        assert_eq!(error.to_string(), expected_error);
    }
}
