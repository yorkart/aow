#!/usr/bin/env bash
set -euo pipefail

# CI supplies a complete, verified asset directory. Published versions are never overwritten.
if [[ $# != 3 ]]; then
    printf '%s\n' 'Usage: publish-github-release.sh VERSION COMMIT_SHA ASSET_DIR' >&2
    exit 1
fi
version=$1
revision=$2
assets=$3
: "${GH_REPO:?GH_REPO must name OWNER/REPO}"
[[ "$version" =~ ^[A-Za-z0-9][A-Za-z0-9._-]*$ && "$version" != latest ]]
[[ "$revision" =~ ^[a-f0-9]{40}$ ]]
for name in aow-linux-x86_64.tar.gz aow-linux-aarch64.tar.gz aow-macos-x86_64.tar.gz aow-macos-aarch64.tar.gz aow-install.sh SHA256SUMS; do
    [[ -s "$assets/$name" ]] || { printf 'Missing release asset: %s\n' "$name" >&2; exit 1; }
done
(
    cd "$assets"
    shasum -a 256 -c SHA256SUMS
)

# A rerun of an old commit must not replace a newer main release as latest.
main_revision=$(gh api "repos/$GH_REPO/git/ref/heads/main" --jq '.object.sha')
if [[ "$main_revision" != "$revision" ]]; then
    printf 'Skipping publication: main has advanced beyond %s.\n' "$revision"
    exit 0
fi

if existing=$(gh release view "$version" --json isDraft,targetCommitish 2>/dev/null); then
    node --input-type=module - "$existing" "$revision" <<'JS'
const [value, revision] = process.argv.slice(2);
const release = JSON.parse(value);
if (!release.isDraft || release.targetCommitish !== revision) {
    throw new Error('Refusing to replace a published release or a draft for another commit');
}
JS
else
    notes=$(mktemp)
    trap 'rm -f -- "$notes"' EXIT
    cat >"$notes" <<EOF
Automated main build at $revision.

Install (Bash, Node.js 20+, Git and tar required):

\`\`\`bash
curl -fsSL https://github.com/$GH_REPO/releases/latest/download/aow-install.sh | bash
\`\`\`

Existing installations: \`aow update\`. For this exact build, append \`-s -- --version $version\` to bash.
Linux uses systemd user services; macOS uses launchd in a logged-in graphical session.
EOF
    gh release create "$version" --draft --target "$revision" --title "AOW $version" --notes-file "$notes"
fi
gh release upload "$version" "$assets/"* --clobber
# Uploads finish while the release is hidden; only then can latest point at it.
gh release edit "$version" --draft=false --latest
printf 'Published https://github.com/%s/releases/tag/%s\n' "$GH_REPO" "$version"
