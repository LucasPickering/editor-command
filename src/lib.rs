//! Get an executable [Command] to open a particular file in the user's
//! configured editor, via the `VISUAL` or `EDITOR` environment variables. See
//! the [editor_command] function for exact details on the behavior.
//!
//! ```
//! // TODO
//! ```
//!
//! ## Resources
//!
//! For more information on the `VISUAL` and `EDITOR` variables, try these
//! resources:
//!
//! TODO better links
//! - [$VISUAL variable](https://bash.cyberciti.biz/guide/$VISUAL_variable)
//! - [$VISUAL vs. $EDITOR variable – what is the difference?](https://bash.cyberciti.biz/guide/$VISUAL_vs._$EDITOR_variable_%E2%80%93_what_is_the_difference%3F)

use std::{env, path::Path, process::Command};

use shellish_parse::ParseOptions;
use thiserror::Error;

/// Get an executable [Command] to open the given file in the user's configured
/// editor. The command will be loaded from one of the following sources, in
/// decreasing precedence:
///
/// - `priority` argument
/// - `VISUAL` environment variable
/// - `EDITOR` environment variable
/// - `default` argument
///
/// The `priority` argument is useful for passing through an app-specific
/// configured command, while `default` is useful for a fallback command in case
/// everything else is undefined.
///
/// After being loaded, the command will be parsed as a shell-like command,
/// using [shellish_parse] (see that crate's documentation for more detail).
pub fn editor_command(
    file: &Path,
    priority: Option<&str>,
    default: Option<&str>,
) -> Result<Command, EditorCommandError> {
    fn parse(input: &str) -> Result<Vec<String>, EditorCommandError> {
        shellish_parse::parse(input, ParseOptions::default())
            .map_err(EditorCommandError::ParseError)
    }

    // Find the first option that has a value. If parsing fails we'll return
    // immediately
    let mut command = priority
        .map(parse)
        .or_else(|| {
            let input = env::var("VISUAL").ok()?;
            Some(parse(&input))
        })
        .or_else(|| {
            let input = env::var("EDITOR").ok()?;
            Some(parse(&input))
        })
        .or_else(|| default.map(parse))
        .ok_or(EditorCommandError::NoCommand)??;

    let mut command = command.drain(..);
    let program = command.next().ok_or(EditorCommandError::EmptyCommand)?;
    let args = command; // Everything left is arguments
    let mut command = Command::new(program);
    command.args(args).arg(file);
    Ok(command)
}

/// Any error that can occur while loading the editor command.
#[derive(Debug, Error)]
pub enum EditorCommandError {
    /// Couldn't find an editor command anywhere
    #[error("VISUAL and EDITOR environment variables are undefined")]
    NoCommand,

    /// The editor command was found, but it's just an empty/whitespace string
    #[error("Editor command is empty")]
    EmptyCommand,

    /// Editor command couldn't be parsed in a shell-like format
    #[error("Invalid editor command: {0}")]
    ParseError(#[source] shellish_parse::ParseError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::ffi::OsStr;

    #[rstest]
    #[case::priority(
        Some("zed"), Some("ted"), Some("fred"), Some("ded"), "zed", &[]
    )]
    #[case::visual(None, Some("ted"), Some("fred"), Some("ded"), "ted", &[])]
    #[case::editor(None, None, Some("fred"), Some("ded"), "fred", &[])]
    #[case::default(None, None, None, Some("ded"), "ded", &[])]
    #[case::with_args(
        Some("ned --wait 60s"), None, None , None, "ned", &["--wait", "60s"]
    )]
    #[case::quotes(
        Some("ned '--single \" quotes' \"--double ' quotes\""),
        None,
        None,
        None,
        "ned",
        &["--single \" quotes", "--double ' quotes"],
    )]
    fn get_editor_command(
        #[case] priority: Option<&str>,
        #[case] env_visual: Option<&str>,
        #[case] env_editor: Option<&str>,
        #[case] default: Option<&str>,
        #[case] expected_program: &str,
        #[case] expected_args: &[&str],
    ) {
        let file_name = "file.yml";
        // Make sure we're not competing with the other tests that want to set
        // these env vars
        let command = {
            let _guard = env_lock::lock_env([
                ("VISUAL", env_visual),
                ("EDITOR", env_editor),
            ]);
            editor_command(Path::new(file_name), priority, default)
        }
        .unwrap();
        let mut expected_args = expected_args.to_owned();
        expected_args.push(file_name);
        assert_eq!(command.get_program(), expected_program);
        assert_eq!(
            command
                .get_args()
                .filter_map(OsStr::to_str)
                .collect::<Vec<_>>()
                .as_slice(),
            expected_args
        );
    }

    /// Test when all options are undefined
    #[test]
    fn get_editor_no_command() {
        let _guard = env_lock::lock_env([
            ("VISUAL", None::<&str>),
            ("EDITOR", None::<&str>),
        ]);
        assert_eq!(
            editor_command(Path::new("file.yml"), None, None)
                .unwrap_err()
                .to_string(),
            "VISUAL and EDITOR environment variables are undefined"
        );
    }

    /// Test when the command exists but is empty
    #[test]
    fn get_editor_empty_command() {
        assert_eq!(
            editor_command(Path::new("file.yml"), Some(""), None)
                .unwrap_err()
                .to_string(),
            "Editor command is empty"
        );
    }

    /// Test when a value can't be parsed as a command string
    #[test]
    fn get_editor_invalid_command() {
        assert_eq!(
            editor_command(
                Path::new("file.yml"),
                Some("'unclosed quote"),
                None
            )
            .unwrap_err()
            .to_string(),
            "Invalid editor command: dangling string"
        );
    }
}
