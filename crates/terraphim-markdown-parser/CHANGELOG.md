# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.20.5] - 2026-10-05

### Fixed

- Republished (1.20.3 and 1.20.4 are taken on crates.io, so this skips to 1.20.5) so that every crates.io dependency is recorded against the sparse index (`sparse+https://index.crates.io/`). Earlier releases in the `terraphim` registry recorded `https://github.com/rust-lang/crates.io-index`, which Cargo treats as a different source from the sparse index, so consumers built serde, ahash and getrandom twice and failed to compile (#80). No code change.

## [0.1.0](https://github.com/terraphim/terraphim-ai/releases/tag/terraphim-markdown-parser-v0.1.0) - 2024-04-29

### Other
- use `Document` and `url` everywhere
- merge article and document
- persistance -> persistence
- * The `server-axum` folder got renamed to `terraphim_server` to align with the crate name. The behavior stays the same.
- First commit into new repo - removing submodules
