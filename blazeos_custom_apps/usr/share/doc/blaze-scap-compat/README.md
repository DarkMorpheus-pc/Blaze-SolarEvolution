# Blaze SCAP Compatibility Adapter (`blaze-scap-compat`)

## Purpose
Blaze SolarEvolution identifies natively via `ID=blazeos`, `ID_LIKE=fedora`,
`BLAZE_BASE_ID=fedora`, `BLAZE_BASE_VERSION_ID=45`, and
`CPE_NAME="cpe:/o:blazeos:solarevolution:5.05.1"`.

Upstream Fedora `scap-security-guide` ships `/usr/share/xml/scap/ssg/content/ssg-fedora-ds.xml`,
whose top-level CPE inventory test checks `/etc/system-release-cpe` for
`^cpe:/o:fedoraproject:fedora:[0-9]+$`. On Blaze SolarEvolution, unmodified
upstream `ssg-fedora-ds.xml` therefore evaluates benchmark rules as
`notapplicable`.

## Design & Auditability
`blaze-scap-compat` leaves upstream `/usr/share/xml/scap/ssg/content/ssg-fedora-ds.xml`
completely unmodified. Instead, `/usr/bin/blazeos-scap-adapt` generates
`/usr/share/blazeos/scap/ssg-blazeos-fedora45-ds.xml` by:
1. Adding CPE entries `cpe:/o:blazeos:solarevolution:5.05.1` (and `5.05`) linked
   to OVAL inventory check `oval:blazeos-installed_OS_is_solarevolution_fedora45:def:1`.
2. Verifying via OVAL that the target host explicitly satisfies ALL of:
   - `ID=blazeos`, `BLAZE_BASE_ID=fedora`, `BLAZE_BASE_VERSION_ID=45` in `/etc/os-release`
   - `/etc/system-release-cpe` matching `cpe:/o:blazeos:solarevolution:5.05*`
   - `fedora-release*` and `blaze-release` RPM packages installed
3. Preserving every rule-level OVAL/SCE applicability check (`grub2`, `audit`,
   `gdm`, `#not_bootc`, etc.) untouched so rule compliance reporting stays
   100% honest with zero artificial score inflation.
