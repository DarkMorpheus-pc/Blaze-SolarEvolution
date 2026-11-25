#!/usr/bin/env bash
# BlazeOS / SolarEvolution 5.05.1 — Format Oncesi Antigravity Bellegi & Surum Yedekleme Betigi
# Pre-format backup utility for Antigravity state, full Git bundle, ISO & evaluation/SBOM archives.
#
# Kullanim / Usage:
#   ./scripts/backup_antigravity_and_release.sh [--dry-run] [--allow-live] <HEDEF_YEDEK_DIZINI>
#
set -euo pipefail
export LC_ALL=C.UTF-8 LANG=C.UTF-8

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

DRY_RUN=0
ALLOW_LIVE=0
DEST_DIR=""

print_usage() {
    cat <<'EOF'
Kullanim / Usage:
  ./scripts/backup_antigravity_and_release.sh [--dry-run] [--allow-live] <HEDEF_YEDEK_DIZINI>

Secenekler / Options:
  --dry-run      Hicbir dosya yazmadan kaynaklari, bos disk alanini ve adimlari dogrular.
  --allow-live   Antigravity acikken canli yedeklemeye izin verir (onerilmez; SQLite/JSONL tutarliligi icin uygulamayi kapatin).
  -h, --help     Bu yardim metnini gosterir.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --allow-live)
            ALLOW_LIVE=1
            shift
            ;;
        -h|--help)
            print_usage
            exit 0
            ;;
        -*)
            echo "[HATA] Bilinmeyen secenek: $1" >&2
            print_usage >&2
            exit 2
            ;;
        *)
            if [[ -n "$DEST_DIR" ]]; then
                echo "[HATA] Birden fazla hedef dizin belirtildi: '$DEST_DIR' ve '$1'" >&2
                exit 2
            fi
            DEST_DIR="$1"
            shift
            ;;
    esac
done

if [[ -z "$DEST_DIR" ]]; then
    echo "[HATA] Hedef yedekleme dizini belirtilmedi (ornegin harici NVMe/USB baglama noktasi)." >&2
    print_usage >&2
    exit 2
fi

for req_cmd in tar zstd git sha256sum df awk; do
    if ! command -v "$req_cmd" >/dev/null 2>&1; then
        echo "[HATA] Gerekli arac bulunamadi: $req_cmd" >&2
        exit 1
    fi
done

AGY_DIR="${HOME}/.gemini/antigravity"
AGY_CFG_DIR="${HOME}/.gemini/config"
SIGNING_DIR="${BLAZE_GNUPGHOME:-${HOME}/.blaze-signing}"
ISO_NAME="Blaze-SolarEvolution-5.05.1-RC-x86_64.iso"
ISO_PATH="${REPO_ROOT}/${ISO_NAME}"
ISO_SHA_PATH="${ISO_PATH}.sha256"
EVAL_DIR="${REPO_ROOT}/solarevolution-evaluation"
HANDOFF_FILE="${REPO_ROOT}/BLAZE_CONTINUITY_HANDOFF.txt"
AGENTS_FILE="${REPO_ROOT}/AGENTS.md"

# 1. Antigravity calisma durumu kontrolu
SELF_PID="$$"
LIVE_PROCS="$(pgrep -u "$(id -u)" -fa 'antigravity|language_server' 2>/dev/null | grep -v -E "^${SELF_PID} |backup_antigravity_and_release\.sh" || true)"
if [[ -n "$LIVE_PROCS" ]]; then
    if [[ "$DRY_RUN" -eq 1 ]]; then
        echo "[UYARI][DRY-RUN] Antigravity su anda acik gorunuyor. Gercek yedekleme oncesi Antigravity'yi kapatin veya --allow-live kullanin."
    elif [[ "$ALLOW_LIVE" -ne 1 ]]; then
        echo "[HATA] Antigravity su anda calisiyor. Konusma gecmisi ve veritabani tutarliligi icin lutfen once Antigravity'yi tamamen kapatip bu betigi normal bir terminalden calistirin." >&2
        echo "       (Acik oturum sirasinda zorla yedek almak isterseniz --allow-live bayragini ekleyebilirsiniz.)" >&2
        exit 1
    else
        echo "[UYARI] --allow-live etkin: Antigravity calisirken canli arsivleme yapiliyor."
    fi
fi

# 2. Kaynak varlik kontrolleri
if [[ ! -d "$AGY_DIR" ]]; then
    echo "[HATA] Antigravity veri dizini bulunamadi: \$HOME/.gemini/antigravity" >&2
    exit 1
fi
if [[ ! -d "${REPO_ROOT}/.git" ]]; then
    echo "[HATA] Git deposu bulunamadi: ${REPO_ROOT}" >&2
    exit 1
fi
if [[ ! -f "$ISO_PATH" ]]; then
    echo "[HATA] ISO imaji bulunamadi: ${ISO_NAME}" >&2
    exit 1
fi
if [[ ! -f "$ISO_SHA_PATH" ]]; then
    echo "[HATA] ISO SHA-256 dosyasi bulunamadi: ${ISO_NAME}.sha256" >&2
    exit 1
fi

AGY_REL_ITEMS=(".gemini/antigravity")
if [[ -d "$AGY_CFG_DIR" ]]; then
    AGY_REL_ITEMS+=(".gemini/config")
fi

echo "=== BlazeOS 5.05.1 & Antigravity Yedekleme Plani ==="
echo "  Mod              : $( [[ "$DRY_RUN" -eq 1 ]] && echo "DRY-RUN (yalnizca dogrulama)" || echo "CANLI YEDEKLEME" )"
echo "  Hedef Dizin      : ${DEST_DIR}"
echo "  Antigravity      : ${AGY_REL_ITEMS[*]} (\$HOME tabanli goreli arsiv)"
echo "  Git Bundle       : blaze-solarevolution-full.bundle (--all)"
echo "  Canli ISO        : ${ISO_NAME} + ${ISO_NAME}.sha256"
echo "  Degerlendirme    : $( [[ -d "$EVAL_DIR" ]] && echo "solarevolution-evaluation-full.tar.zst (SBOM + raw-results dahil)" || echo "yok" )"
echo "  RPM Imza Anahtari: $( [[ -d "$SIGNING_DIR" ]] && echo "blaze-signing-keyring.tar.zst (0600)" || echo "mevcut degil (atlandi)" )"
echo "  Devamlilik Belge : $( [[ -f "$HANDOFF_FILE" ]] && echo "BLAZE_CONTINUITY_HANDOFF.txt + AGENTS.md" || echo "AGENTS.md" )"

if [[ "$DRY_RUN" -eq 1 ]]; then
    ISO_BYTES="$(stat -c %s "$ISO_PATH")"
    echo "[DRY-RUN] ISO boyutu: ${ISO_BYTES} bayt."
    echo "[DRY-RUN] Tum on kosullar basariyla dogrulandi. Hicbir dosya yazilmadi."
    exit 0
fi

mkdir -p "$DEST_DIR"
DEST_ABS="$(cd "$DEST_DIR" && pwd)"
case "${DEST_ABS}/" in
    "${REPO_ROOT}/"*)
        echo "[HATA] Hedef yedek dizini Git calisma alani (${REPO_ROOT}) disinda olmalidir." >&2
        exit 2
        ;;
esac
CHECKSUM_TARGETS=()

echo "[1/6] Antigravity bellegi ve eklenti yapilandirmasi arsivleniyor (tar --zstd)..."
(
    umask 0077
    tar --zstd -cf "${DEST_ABS}/antigravity-state-backup.tar.zst" \
        --exclude='.gemini/antigravity/*.sock' \
        --exclude='.gemini/antigravity/*.lock' \
        -C "$HOME" "${AGY_REL_ITEMS[@]}"
)
chmod 0600 "${DEST_ABS}/antigravity-state-backup.tar.zst"
CHECKSUM_TARGETS+=("antigravity-state-backup.tar.zst")

if [[ -d "$SIGNING_DIR" ]]; then
    echo "[2/6] Blaze RPM EdDSA/GPG imzalama anahtarligi arsivleniyor (0600)..."
    SIGNING_PARENT="$(dirname "$SIGNING_DIR")"
    SIGNING_BASE="$(basename "$SIGNING_DIR")"
    (
        umask 0077
        tar --zstd -cf "${DEST_ABS}/blaze-signing-keyring.tar.zst" \
            --exclude='S.gpg-agent*' \
            -C "$SIGNING_PARENT" "$SIGNING_BASE"
    )
    chmod 0600 "${DEST_ABS}/blaze-signing-keyring.tar.zst"
    CHECKSUM_TARGETS+=("blaze-signing-keyring.tar.zst")
else
    echo "[2/6] RPM imzalama anahtarligi bulunamadi, atlandi."
fi

echo "[3/6] Tam Git gecmisi paketleniyor (git bundle create --all)..."
git -C "$REPO_ROOT" bundle create "${DEST_ABS}/blaze-solarevolution-full.bundle" --all
git -C "$REPO_ROOT" bundle verify "${DEST_ABS}/blaze-solarevolution-full.bundle" >/dev/null
CHECKSUM_TARGETS+=("blaze-solarevolution-full.bundle")

echo "[4/6] Blaze-SolarEvolution-5.05.1-RC ISO ve dogrulama belgesi kopyalaniyor..."
cp -f "$ISO_PATH" "${DEST_ABS}/${ISO_NAME}"
cp -f "$ISO_SHA_PATH" "${DEST_ABS}/${ISO_NAME}.sha256"
CHECKSUM_TARGETS+=("${ISO_NAME}" "${ISO_NAME}.sha256")

if [[ -f "$AGENTS_FILE" ]]; then
    cp -f "$AGENTS_FILE" "${DEST_ABS}/AGENTS.md"
    CHECKSUM_TARGETS+=("AGENTS.md")
fi
if [[ -f "$HANDOFF_FILE" ]]; then
    cp -f "$HANDOFF_FILE" "${DEST_ABS}/BLAZE_CONTINUITY_HANDOFF.txt"
    CHECKSUM_TARGETS+=("BLAZE_CONTINUITY_HANDOFF.txt")
fi

if [[ -d "$EVAL_DIR" ]]; then
    echo "[5/6] Teknik degerlendirme ve ham SBOM kanitlari arsivleniyor (tar --zstd)..."
    tar --zstd -cf "${DEST_ABS}/solarevolution-evaluation-full.tar.zst" \
        -C "$REPO_ROOT" "solarevolution-evaluation"
    CHECKSUM_TARGETS+=("solarevolution-evaluation-full.tar.zst")
else
    echo "[5/6] solarevolution-evaluation dizini bulunamadi, atlandi."
fi

echo "[6/6] SHA256SUMS.txt uretiliyor ve sha256sum -c ile dogrulaniyor..."
(
    cd "$DEST_ABS"
    sha256sum "${CHECKSUM_TARGETS[@]}" > SHA256SUMS.txt
    sha256sum -c SHA256SUMS.txt
)
sync

echo "=== YEDEKLEME TAMAMLANDI VE DOGRULANDI ==="
echo "Hedef dizin: ${DEST_ABS}"
ls -lh "$DEST_ABS"
