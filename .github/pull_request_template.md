## What and why
<!-- What does this change, and what problem does it solve? Link the issue if there is one (Fixes #123). -->

## How it was tested
<!-- Operating system, DJ software and controller if they matter, and what you checked. -->

## Checklist
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass
- [ ] New user-visible texts are in all locale files (`npm run check-i18n` in `apps/desktop`)
- [ ] User-visible changes are noted in `CHANGELOG.md` under `## Unreleased`
- [ ] No passwords, webhooks, keys or other secrets in the diff
