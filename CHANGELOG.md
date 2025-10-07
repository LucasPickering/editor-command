# Changelog

All user-facing changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased] - ReleaseDate

### Breaking

- `EditorBuilder::build` now returns an `Editor` instead of a command
  - Use `Editor::open` to get the command
- `EditorBuilder::path` and `EditorBuilder::paths` have been removed in favor of `Editor::open`
  - There is no longer a way to open more than one path at a time
- `EditorBuilder::source` renamed to `EditorBuilder::string`

### Added

- Add `Editor::open_at`, which opens to a specific line/column

## [1.0.0] - 2024-12-27

### Changed

- Replace `shellish_parse` with `shell-words` for parsing

## [0.1.1] - 2024-09-05

## [0.1.0] - 2024-08-16

Initial release!
