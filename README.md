# terraphim-core

Foundational Rust crates for Terraphim's editor and knowledge-graph search.

- `terraphim_types` — typed concepts, documents, roles and synonyms
- `terraphim-markdown-parser` — Markdown parsing, bounded traversal and stable blocks
- `terraphim_automata` — text matching, autocomplete and thesaurus construction
- `terraphim_rolegraph_core` — portable trigger indexing and symbolic embeddings
- `terraphim_rolegraph` — knowledge-graph construction and ranking

The workspace resolves dependencies from crates.io and builds without private
registry credentials. Authored terminology can stay in Markdown:

```markdown
# Risk

synonyms:: concern, uncertainty
```

Terraphim's automata parser maps these terms to the same typed concept. The
editor and search clients use that shared vocabulary when proposing alternatives
and retrieving related passages.

Build and test with Rust 1.96:

```sh
cargo test --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
```

Crate archives include their own licenses and test fixtures. The release workflow
checks package contents and publishes reviewed version tags through crates.io
trusted publishing; new crate names use an explicit first-publication bootstrap.

Source: [terraphim-core](https://github.com/terraphim/terraphim-core).
Licensed under Apache-2.0; see each crate's `LICENSE`.
