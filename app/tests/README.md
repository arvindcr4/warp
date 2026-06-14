# `app/tests`

This directory holds **integration test fixtures and infrastructure** for the
`app` crate — it is not itself a Rust test target. There are no `#[test]`
functions here; the `app` crate's integration tests live in
`crates/integration/tests/`.

## Layout

- `ssh/` — Dockerfile and instructions for running the Warp app inside an SSH
  session inside a Docker container. Used by the "Build image and start
  container for SSH testing" workflow.

If you add a new fixture, drop it under a subdirectory named after its
purpose (e.g. `ssh/`, `notebook/`, `theme/`) and link to it from this README
so the next reviewer can find it.
