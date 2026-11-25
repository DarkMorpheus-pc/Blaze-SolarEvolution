#!/usr/bin/env bash
# Install the freshly built packages from the signed local repo into a clean Fedora
# container, with gpgcheck and repo_gpgcheck enabled, and smoke-test them.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
FEDORA_IMAGE="${FEDORA_IMAGE:-registry.fedoraproject.org/fedora:45}"

podman run --rm -v "$HERE/out/repo:/repo:ro,Z" "$FEDORA_IMAGE" bash -euo pipefail -c '
  cat > /etc/yum.repos.d/blaze-test.repo <<REPO
[blaze-test]
name=Blaze test
baseurl=file:///repo
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=file:///repo/RPM-GPG-KEY-blaze
REPO
  dnf -y -q install blaze-release blaze-sentinel blaze-tools blaze-recovery blaze-continuum blaze-house solarui solarui-config blaze-oobe blaze-firehub blaze-scap-compat blaze-akrep 2>&1 | tail -8
  echo "--- installed:"
  rpm -q blaze-release blaze-sentinel blaze-tools blaze-recovery blaze-continuum blaze-house solarui solarui-config blaze-oobe blaze-firehub blaze-scap-compat blaze-akrep blaze-akrep-daemon blaze-akrep-ui
  echo "--- sentinel & akrep smoke test:"
  blaze-sentinel status | head -6
  akrepd --version
  akrep-ui --version
  akrepd --oneshot-json | grep -q "\"mutators_enabled\": false" || { echo "FATAL: akrepd mutators_enabled must be false"; exit 1; }
  echo "--- blaze.repo shipped by blaze-release:"
  cat /usr/share/dnf5/repos.d/blaze.repo
  echo "--- identity persistence & releasever=45 checks:"
  grep -qxF "PRETTY_NAME=\"Blaze SolarEvolution 5.05.1\"" /etc/os-release
  grep -qxF "Blaze SolarEvolution release 5.05.1 (SolarEvolution)" /etc/redhat-release
  grep -qx "BLAZE_BASE_ID=fedora" /etc/os-release
  grep -qx "BLAZE_BASE_VERSION_ID=45" /etc/os-release
  grep -qx "45" /etc/dnf/vars/releasever
  dnf5 --dump-variables | grep -qx "releasever = 45" || { dnf5 --dump-variables | grep -E "^releasever"; exit 1; }
  grep -qF "excludepkgs=fedora-release*" /etc/dnf/libdnf5.conf.d/10-blaze-identity.conf
  dnf5 check --dependencies
  echo "--- rpm -V first-party suite:"
  rpm -V blaze-sentinel blaze-continuum blaze-house blaze-recovery blaze-tools solarui solarui-config blaze-oobe blaze-firehub blaze-scap-compat blaze-akrep blaze-akrep-daemon blaze-akrep-ui blaze-release
  echo "--- blaze-akrep upgrade/remove/reinstall roundtrip:"
  rpm -V blaze-akrep-daemon blaze-akrep-ui
  dnf -y -q remove blaze-akrep blaze-akrep-ui blaze-akrep-daemon >/dev/null
  dnf -y -q install blaze-akrep >/dev/null
  rpm -V blaze-akrep blaze-akrep-daemon blaze-akrep-ui
  echo "--- akrep RPM install/remove/reinstall + rpm -V OK!"
  echo "--- blaze-scap-compat offline XML & datastream smoke test:"
  dnf -y -q install scap-security-guide openscap-scanner >/dev/null
  test -s /usr/share/blazeos/scap/ssg-blazeos-fedora45-ds.xml
  oscap info /usr/share/blazeos/scap/ssg-blazeos-fedora45-ds.xml | head -n 12
  oscap xccdf eval --verbose INFO --profile xccdf_org.ssgproject.content_profile_standard --rule xccdf_org.ssgproject.content_rule_rpm_verify_hashes /usr/share/blazeos/scap/ssg-blazeos-fedora45-ds.xml >/tmp/scap_test.log 2>&1 || true
  grep -qE "oval:blazeos-installed_OS_is_solarevolution_fedora45:def:1.*evaluated as true" /tmp/scap_test.log || { tail -n 30 /tmp/scap_test.log; echo "FATAL: blaze-scap-compat OVAL did not evaluate true"; exit 1; }
  grep -qE "Result[[:space:]]+(pass|fail)" /tmp/scap_test.log || { tail -n 30 /tmp/scap_test.log; echo "FATAL: blaze-scap-compat rule returned non-applicable"; exit 1; }
  echo "--- blaze-scap-compat applicability & rule evaluation OK!"
  echo "--- tamper test (must be detected):"
  cp /repo/blaze-sentinel-*.rpm /tmp/x.rpm
  sz=$(stat -c %s /tmp/x.rpm)
  printf X | dd of=/tmp/x.rpm bs=1 seek=$((sz/2)) conv=notrunc 2>/dev/null
  if rpm -K /tmp/x.rpm; then echo UNEXPECTED-PASS; exit 1; else echo tamper-detected-OK; fi
'
