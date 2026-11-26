#!/usr/bin/env bash
# Publish packaging/out/repo to the gh-pages branch (served by GitHub Pages at
# https://darkmorpheus-pc.github.io/Blaze-SolarEvolution/repo/).
# The branch holds only the latest repo snapshot (orphan commit, force push) so
# binary history does not accumulate in the repository.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
REPO_DIR="$HERE/out/repo"
REMOTE="${BLAZE_REMOTE:-$(git -C "$ROOT" remote get-url origin)}"

[ -f "$REPO_DIR/repodata/repomd.xml.asc" ] || { echo "run build-rpms.sh first (signed repomd missing)" >&2; exit 1; }
shopt -s nullglob
rpms=("$REPO_DIR"/*.rpm)
[ "${#rpms[@]}" -gt 0 ] || { echo "no RPM packages found in $REPO_DIR" >&2; exit 1; }
for rpm_file in "${rpms[@]}"; do
  size_bytes="$(stat -c '%s' "$rpm_file")"
  if [ "$size_bytes" -ge 104857600 ]; then
    echo "FATAL: RPM exceeds GitHub 100 MiB file limit ($size_bytes bytes): $rpm_file" >&2
    exit 1
  fi
  sig="$(rpm -qp --qf '%{OPENPGP:pgpsig}|%{SIGPGP:pgpsig}|%{RSAHEADER:pgpsig}|%{DSAHEADER:pgpsig}' "$rpm_file" 2>/dev/null || echo '(none)|(none)|(none)|(none)')"
  if [ "$sig" = "(none)|(none)|(none)|(none)" ] || [ -z "$sig" ]; then
    echo "FATAL: refusing to publish unsigned RPM: $rpm_file" >&2
    exit 1
  fi
done

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT
mkdir -p "$TMP/repo"
cp -a "$REPO_DIR/." "$TMP/repo/"
python3 - <<'PY' "$TMP/repo"
import datetime, json, os, subprocess, sys
repo_dir = sys.argv[1]
pkgs = {}
for fn in sorted(os.listdir(repo_dir)):
    if not fn.endswith(".rpm"):
        continue
    out = subprocess.check_output(
        ["rpm", "-qp", "--qf", "%{NAME}\t%{VERSION}-%{RELEASE}", os.path.join(repo_dir, fn)],
        text=True,
        stderr=subprocess.DEVNULL,
    ).strip()
    if "\t" in out:
        name, vr = out.split("\t", 1)
        pkgs[name] = vr
core_packages = [
    p for p in (
        "blaze-release",
        "blaze-akrep",
        "solarui",
        "solarui-config",
        "blaze-recovery",
        "blaze-tools",
        "blaze-sentinel",
        "blaze-scap-compat",
    )
    if p in pkgs
]
manifest = {
    "product": "Blaze SolarEvolution",
    "series": "5.05.1",
    "base_fedora": "45",
    "published_at": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "repo_url": "https://darkmorpheus-pc.github.io/Blaze-SolarEvolution/repo/",
    "github_repo": "DarkMorpheus-pc/Blaze-SolarEvolution",
    "core_packages": core_packages,
    "packages": pkgs,
    "upgrade_target": None,
}
if os.environ.get("BLAZE_UPGRADE_TARGET_VER"):
    manifest["upgrade_target"] = {
        "series": os.environ.get("BLAZE_UPGRADE_SERIES", "6.0"),
        "target_releasever": os.environ["BLAZE_UPGRADE_TARGET_VER"],
        "ready": os.environ.get("BLAZE_UPGRADE_READY", "false").lower() == "true",
        "min_blaze_release": pkgs.get("blaze-release", "5.05.1-1.blaze"),
        "notes": os.environ.get("BLAZE_UPGRADE_NOTES", "Yeni Blaze SolarEvolution ana surum yukseltmesi hazir."),
    }
with open(os.path.join(repo_dir, "channel.json"), "w", encoding="utf-8") as f:
    json.dump(manifest, f, indent=2, ensure_ascii=False)
PY
touch "$TMP/.nojekyll"
cat > "$TMP/index.html" <<'HTML'
<!doctype html><meta charset="utf-8"><title>Blaze SolarEvolution package repository</title>
<h1>Blaze SolarEvolution package repository</h1>
<p>Install <code>blaze-release</code> to enable it, or add <code>repo/</code> to <code>/etc/yum.repos.d/</code>.</p>
<p>Signing key: <a href="repo/RPM-GPG-KEY-blaze">RPM-GPG-KEY-blaze</a></p>
<ul><li><a href="repo/">Packages</a></li></ul>
HTML

cd "$TMP"
git init -q -b gh-pages
git add -A
git -c user.name="Blaze Release" -c user.email="release@blazeos.com.tr" commit -q -m "Publish Blaze repo snapshot $(date -u +%Y-%m-%dT%H:%MZ)"
git push -q --force "$REMOTE" gh-pages
echo "published to gh-pages of $REMOTE"
