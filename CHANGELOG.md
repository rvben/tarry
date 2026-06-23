# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/).




## [0.1.3](https://github.com/rvben/tarry/compare/v0.1.2...v0.1.3) - 2026-06-23

### Fixed

- **cli**: remove the redundant top-level run alias in favor of gh run ([5a6ae44](https://github.com/rvben/tarry/commit/5a6ae4446aa77c33e0116f836c39bdb3948b0b0a))

## [0.1.2](https://github.com/rvben/tarry/compare/v0.1.1...v0.1.2) - 2026-06-23

### Fixed

- **run**: don't infer current branch when --workflow is given ([d6b4856](https://github.com/rvben/tarry/commit/d6b48566d3ef8084607b2faa31e81d095c6785d1))

## [0.1.1](https://github.com/rvben/tarry/compare/v0.1.0...v0.1.1) - 2026-06-23

### Fixed

- **run**: skip stale prior run when resolving the latest workflow run ([492cf00](https://github.com/rvben/tarry/commit/492cf000c603b45fc3f0dd1d9c919b32b1f809eb))

## [0.1.0] - 2026-06-12

### Added

- clispec v0.2 schema command ([59ffff8](https://github.com/rvben/tarry/commit/59ffff82c83e2902384dde3422a34abd1167edec))
- cli wiring, output format detection, usage and environment exit codes ([8f9bac3](https://github.com/rvben/tarry/commit/8f9bac3b47cc0af51c0a9d0c44090cff8cd91e98))
- github actions run probe with failure digest ([9f7d4f2](https://github.com/rvben/tarry/commit/9f7d4f2ab61468d3a1ae69d061107b526cb9558a))
- cmd probe with ok-output override ([6d26d23](https://github.com/rvben/tarry/commit/6d26d232505e3f52596e2e4e28d5c59e422fc14b))
- http probe with status, contains, and json-path matchers ([45ad3ba](https://github.com/rvben/tarry/commit/45ad3baf481155ee1238deebd4ba6b30b8f3c0ea))
- tcp probe ([ea09628](https://github.com/rvben/tarry/commit/ea09628bbb9dfb721fdfc6d49e3fab75ec24e085))
- file probe with contains and regex matchers ([8598375](https://github.com/rvben/tarry/commit/8598375671ccb1c58fb31a437af8474dfa6ce317))
- verdict rendering with clispec outcome exit codes ([66955bc](https://github.com/rvben/tarry/commit/66955bce3db8ce6332b053295ac4f7211dc907d0))
- probe trait and poll engine with timeout and backoff ([7e650eb](https://github.com/rvben/tarry/commit/7e650eb27b7c0426959f147608d0ce30cdc3cee8))

### Fixed

- file existence without utf-8 decode, multi-address tcp connect, current-branch run lookup ([70eba35](https://github.com/rvben/tarry/commit/70eba355603d6ed44405e7d56f52ff3f4083d366))
