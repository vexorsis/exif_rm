# Project: exif_rm

A Rust library + CLI for stripping metadata from files, with UniFFI mobile bindings.

## Preferences

- **SOCKS5 proxy is configured** for this repo (`socks5://127.0.0.1:1080`) via `git config --local http.proxy`. If git push fails with network errors, the proxy should handle it. If the proxy isn't running, use `gh` API as fallback.
- **Maven Central publishing** uses the Vanniktech plugin. Run `./gradlew :library:publishAndReleaseToMavenCentral` from the `android/` directory. Credentials are in `local.properties` (local) or `ORG_GRADLE_PROJECT_*` env vars (CI). The groupId is `io.github.wangpeiyan`.
