# Project: exif_rm

A Rust library + CLI for stripping metadata from files, with UniFFI mobile bindings.

## Preferences

- **SOCKS5 proxy is configured** for this repo (`socks5://127.0.0.1:1080`) via `git config --local http.proxy`. If git push fails with network errors, the proxy should handle it. If the proxy isn't running, use `gh` API as fallback.
- **Maven Central publishing** uses the Vanniktech plugin. Run `./gradlew :library:publishAndReleaseToMavenCentral` from the `android/` directory. Signing credentials go in `~/.gradle/gradle.properties` as `signingInMemoryKey`, `signingInMemoryKeyId`, `signingInMemoryKeyPassword` (or `ORG_GRADLE_PROJECT_signingInMemoryKey*` env vars for CI). Sonatype tokens go as `mavenCentralUsername`/`mavenCentralPassword`. The groupId is `io.github.wangpeiyan`.
- **Version bumps** must update all three locations together: `Cargo.toml` (`version`), `android/gradle.properties` (`VERSION_NAME`), and the git tag (`v<X>.<Y>.<Z>`). The Maven publish reads `VERSION_NAME`, not the git tag — forgetting to bump `gradle.properties` will cause a "already exists" rejection from Maven Central.
