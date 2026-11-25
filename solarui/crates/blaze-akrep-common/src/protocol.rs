pub const AKREP_PRIV_BUS_NAME: &str = "tr.com.blazeos.Akrep1.Privileged";
pub const AKREP_PRIV_OBJECT_PATH: &str = "/tr/com/blazeos/Akrep1/Privileged";
pub const AKREP_PRIV_INTERFACE_NAME: &str = "tr.com.blazeos.Akrep1.PrivilegedManager";

pub const POLKIT_SELINUX_DIAGNOSE: &str = "tr.com.blazeos.Akrep1.selinux.diagnose";
pub const POLKIT_SELINUX_MANAGE: &str = "tr.com.blazeos.Akrep1.selinux.manage";
pub const POLKIT_FIREWALL_MANAGE: &str = "tr.com.blazeos.Akrep1.firewall.manage";
pub const POLKIT_FLATPAK_SYSTEM_MANAGE: &str = "tr.com.blazeos.Akrep1.flatpak.system-manage";
pub const POLKIT_QUARANTINE_MANAGE: &str = "tr.com.blazeos.Akrep1.quarantine.manage";
pub const POLKIT_PROFILE_APPLY: &str = "tr.com.blazeos.Akrep1.profile.apply";
pub const POLKIT_PACKAGES_INSTALL: &str = "tr.com.blazeos.Akrep1.packages.install-optional";

pub const ALL_POLKIT_ACTIONS: [&str; 7] = [
    POLKIT_SELINUX_DIAGNOSE,
    POLKIT_SELINUX_MANAGE,
    POLKIT_FIREWALL_MANAGE,
    POLKIT_FLATPAK_SYSTEM_MANAGE,
    POLKIT_QUARANTINE_MANAGE,
    POLKIT_PROFILE_APPLY,
    POLKIT_PACKAGES_INSTALL,
];

pub const DEFAULT_PLAN_TTL_SECS: i64 = 300;
