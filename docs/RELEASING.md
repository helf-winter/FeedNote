# FeedNote release process

FeedNote uses Tauri's signed updater and GitHub Releases. The updater endpoint is:

```text
https://github.com/helf-winter/FeedNote/releases/latest/download/latest.json
```

## Trust boundary

- The updater public key is committed in `src-tauri/tauri.conf.json`.
- `data/updater.key` and `data/updater-signing.env` are local secrets and must never be committed.
- The same private key and password are stored as GitHub Actions secrets named `TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
- Losing the private key prevents existing installations from accepting future updates. Leaking it requires rotating the public key through an already trusted release.
- The Tauri update signature verifies release integrity. It does not provide Windows Authenticode reputation, so Windows may still show SmartScreen until a code-signing certificate is configured.

## Publish a version

1. Update the same semantic version in `package.json`, `src-tauri/Cargo.toml`, and `src-tauri/tauri.conf.json`.
2. Run `./scripts/test.ps1` and `./scripts/build.ps1`.
3. Commit and push the changes.
4. Create and push the matching tag, for example `git tag v0.3.0` and `git push origin v0.3.0`.
5. The `Release FeedNote` workflow tests the source, builds the NSIS installer and signature, and publishes `latest.json`.

The workflow rejects a tag whose version differs from either application manifest. Never replace a published release asset in place; publish a higher version instead.
