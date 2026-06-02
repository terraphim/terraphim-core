# terraphim-core

Foundational crates for the Terraphim AI platform, extracted from the
`terraphim-ai` monorepo (Gitea #1910 polyrepo split):

- `terraphim_types` -- shared type definitions
- `terraphim_automata` -- text matching, autocomplete, thesaurus
- `terraphim_rolegraph` -- knowledge-graph implementation
- `terraphim-markdown-parser` -- markdown parsing utilities
- `terraphim_test_utils` -- test helpers

Published to the Terraphim private Cargo registry (`terraphim` org on
git.terraphim.cloud). Consumers add the registry to `.cargo/config.toml`:

```toml
[registries.terraphim]
index = "sparse+https://git.terraphim.cloud/api/packages/terraphim/cargo/"
```
