use blaze_sentinel_core::AkrepKeyValue;
use std::fs;
use std::path::Path;

#[allow(dead_code)]
pub struct SecureBootTelemetry {
    pub uefi_boot: bool,
    pub secure_boot_enabled: Option<bool>,
    pub tpm_present: bool,
    pub tpm_version_tr: String,
    pub tpm_version_en: String,
    pub iommu_active: bool,
    pub metrics: Vec<AkrepKeyValue>,
}

const EFI_SECURE_BOOT_VAR: &str =
    "/sys/firmware/efi/efivars/SecureBoot-8be4df61-93ca-11d2-aa0d-00e098032b8c";

pub fn collect_secureboot_and_tpm() -> SecureBootTelemetry {
    let uefi_boot = Path::new("/sys/firmware/efi").exists();
    let secure_boot_enabled = if uefi_boot {
        fs::read(EFI_SECURE_BOOT_VAR).ok().and_then(|bytes| {
            bytes.last().and_then(|b| match *b {
                1 => Some(true),
                0 => Some(false),
                _ => None,
            })
        })
    } else {
        None
    };

    let tpm_ver_raw = fs::read_to_string("/sys/class/tpm/tpm0/tpm_version_major")
        .ok()
        .map(|s| s.trim().to_string());
    let tpm_present = tpm_ver_raw.as_deref() == Some("2")
        || Path::new("/dev/tpmrm0").exists()
        || Path::new("/sys/class/tpm/tpm0").exists();
    let (tpm_version_tr, tpm_version_en) = match tpm_ver_raw {
        Some(v) if !v.is_empty() => (format!("TPM {}.0", v), format!("TPM {}.0", v)),
        _ if tpm_present => ("TPM Mevcut".to_string(), "TPM Present".to_string()),
        _ => (
            "TPM Yok / Devre Dışı".to_string(),
            "TPM None / Disabled".to_string(),
        ),
    };

    let iommu_active = fs::read_dir("/sys/class/iommu")
        .map(|mut d| d.next().is_some())
        .unwrap_or(false);

    let sb_text = match (uefi_boot, secure_boot_enabled) {
        (false, _) => "Legacy BIOS (UEFI yok)".to_string(),
        (true, Some(true)) => "Etkin (UEFI Secure Boot ON)".to_string(),
        (true, Some(false)) => "Devre Dışı (UEFI Secure Boot OFF)".to_string(),
        (true, None) => "Erişilemiyor / Bilinmiyor".to_string(),
    };

    let metrics = vec![
        AkrepKeyValue {
            label_tr: "Önyükleme Aygıt Yazılımı".to_string(),
            label_en: "Boot Firmware".to_string(),
            value: if uefi_boot {
                "UEFI".to_string()
            } else {
                "Legacy BIOS".to_string()
            },
        },
        AkrepKeyValue {
            label_tr: "UEFI Secure Boot".to_string(),
            label_en: "UEFI Secure Boot".to_string(),
            value: sb_text,
        },
        AkrepKeyValue {
            label_tr: "TPM Güvenlik Modülü".to_string(),
            label_en: "TPM Module".to_string(),
            value: tpm_version_tr.clone(),
        },
        AkrepKeyValue {
            label_tr: "IOMMU DMA Koruması".to_string(),
            label_en: "IOMMU DMA Isolation".to_string(),
            value: if iommu_active {
                "Aktif".to_string()
            } else {
                "Pasif / Sanal Donanım".to_string()
            },
        },
    ];

    SecureBootTelemetry {
        uefi_boot,
        secure_boot_enabled,
        tpm_present,
        tpm_version_tr,
        tpm_version_en,
        iommu_active,
        metrics,
    }
}
