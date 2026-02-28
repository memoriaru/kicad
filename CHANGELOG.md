# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/).

## [Unreleased]

### Added
- Initial release
- S-expression lexer with support for:
  - Parentheses `(` `)`
  - Strings (double and single quoted)
  - Numbers (integers and floats, positive and negative)
  - Booleans (`yes`/`no`/`true`/`false`)
  - Identifiers (symbols)
  - Comments (`;` line comments,- S-expression parser with support for:
  - Nested lists
  - All atom types
- IR (Intermediate Representation) with:
  - `Schematic` structure
  - `Symbol` and `SymbolInstance` definitions
  - `Net`, `Wire`, `Label`, `Junction` definitions
  - `Pin` and `PinInstance` definitions
  - `Paper`, `TitleBlock`, `Metadata` structures
- JSON5 code generator with:
  - Configurable indentation
  - Comment support
  - Structured output format
- CLI tool with options:
  - `-o, --output` - Output file path
  - `-i, --indent` - Indentation size
  - `--no-comments` - Exclude comments
  - `--validate` - Validate input only
  - `--debug-ast` - Print parsed AST
  - `-v, --verbose` - Verbose output

### Changed

### Fixed

### Security

### Deprecated

### Removed

[Unreleased]: https://github.com/olivierlacan/keep-a-changelog
