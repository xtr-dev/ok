# The ok guide

These pages are the user guide published at https://okrun.dev/docs/. `crates/okdocs` turns them into HTML (`cargo run -p okdocs -- build docs _site`), and `cargo test -p okdocs` runs every code sample (`OKDOCS_PAGE=slug` checks one page). The order of the pages is `pages.txt`; the code fences are described at the top of `crates/okdocs/src/lib.rs`. `SPEC.md` stays the normative definition: the guide explains and shows, and links to the spec for the fine print.
