#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:?Usage: $0 <version>  (e.g. 0.1.0-alpha.6)}"

gh issue create \
  --title "Release v${VERSION}" \
  --label "release" \
  --body "$(cat <<EOF
## Release checklist for v${VERSION}

Follow this **exact** sequence. Do not skip steps.

- [ ] All work is merged to main, CI is green
- [ ] Decide version: edit Cargo.toml \`version = "${VERSION}"\`
- [ ] Update snapshots: \`cargo insta test --accept\`
- [ ] Run full suite: \`cargo test --all-features && cargo clippy && cargo fmt --check\`
- [ ] Commit: \`git commit -am "chore: bump version to ${VERSION}"\`
- [ ] Push: \`git push origin main\`
- [ ] Wait for CI to pass on main (check GitHub Actions)
- [ ] Tag: \`git tag v${VERSION}\`
- [ ] Push tag: \`git push origin v${VERSION}\`
- [ ] Watch the Release workflow on GitHub Actions
EOF
)"
