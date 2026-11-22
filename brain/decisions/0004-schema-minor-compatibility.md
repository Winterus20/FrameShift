---
id: 0004
status: accepted
created: 2026-11-22
valid_from: 2026-11-22
tags: [schema, compatibility, versioning]
supersedes: null
---

# 0004 — Şema Sürümünde Minor Geriye Uyumluluğu (0.2.0)

## Bağlam

Kalıcı proje modeli `schema_version` alanıyla sürümlenir ve yükleyici yalnızca uyumlu sürümleri kabul eder (`crates/frameshift-schema/src/project.rs`). Uygulama öncesinde `CURRENT_SCHEMA_VERSION = 0.1.0` idi ve `is_compatible_with_current` **tam minor eşitliği** istiyordu (`major == && minor ==`).

Bu kural iki sorun üretti:

1. **Spec ile çelişki:** [project-schema.md](../specs/project-schema.md) sürüm maddesi `0.2.0` ve üzerini "geriye dönük uyumlu minor genişlemeler" olarak tanımlıyor ve kabul kriteri `0.1.0` projelerinin varsayılanlarla yüklenmesini şart koşuyordu. Katı minor eşitliği bu maddeyi uygulanamaz kılıyordu.
2. **Kaçınılmaz ikilem:** Herhangi bir minor artışı, mevcut tüm `0.1.0` projelerini `UnsupportedSchemaVersion` ile reddediyordu. Bu yüzden yeni yetenekler (fade, burst, scrim, reframe, ribbon, autocut vb.) ya `0.1.0` içine sıkıştırılıyor ya da kullanıcı projeleri kırılıyordu. Yerel `local_media/` projeleri (`asta_amv_project.json`, `asta_devil_union_project.json`) `"schema_version": "0.1.0"` taşır; yani kırılma gerçek dosyaları etkilerdi.

Tüm opsiyonel alanlar `#[serde(default)]` ile tanımlandığı için eski JSON'lar zaten alan eksikliğinden dolayı yüklenebilir durumdadır; engel yalnızca sürüm kapısıdır.

## Karar

1. **Mevcut şema sürümü `0.2.0`'dır.** `CURRENT_SCHEMA_VERSION` `{ major: 0, minor: 2, patch: 0 }` olur; yeni projeler bu sürümle yazılır.
2. **Yükleme kuralı "same major, minor <= current" olur.** `is_compatible_with_current` şu koşulu döndürür: `self.major == CURRENT_SCHEMA_VERSION.major && self.minor <= CURRENT_SCHEMA_VERSION.minor`.
   - `0.1.0` projeleri yüklenir (eski minor).
   - `0.2.0` projeleri yüklenir (mevcut minor).
   - `0.3.0` reddedilir (daha yeni minor; bilinmeyen alanlar taşıyabilir).
   - `1.0.0` reddedilir (farklı major).
3. **Patch bileşeni uyumluluğu etkilemez.** SemVer patch'i kalıcı biçimi değiştirmeyen hata düzeltmesidir; major/minor kuralı geçtiği sürece herhangi bir patch kabul edilir (ör. `0.1.9`, `0.2.7`).
4. **Geriye uyumluluk serde varsayılanlarına dayanır.** Bir minor artışında eklenen her opsiyonel alan `#[serde(default)]` (veya eşdeğeri) taşımak zorundadır; bu kural ihlal edilirse eski projeler yüklenemez.
5. **Spec ve ADR izi:** [project-schema.md](../specs/project-schema.md) `status: locked` kalır; sürüm/uyumluluk maddeleri bu ADR'ye göre düzeltilir ve dosyaya "amended by ADR-0004" notu eklenir.

## Gerekçe

- Spec'in `0.2.0` tanımı ve `0.1.0` kabul kriteri ancak gevşetilmiş kuralla birlikte anlamlı olur; ADR bu çelişkiyi kapatır.
- Tüm opsiyonel alanlar zaten varsayılanlı olduğundan, eski minor'u kabul etmek yeni bir risk sınıfı açmaz: eksik alanlar varsayılana düşer.
- Daha yeni minor'u reddetmek bilinçli bir korumadır: bu build'in tanımadığı alanlar sessizce düşürülürse kullanıcı verisi kaybolur. Sessiz veri kaybı yerine açık hata tercih edilir.
- Major eşitliği, kök yapı yeniden düzenlendiğinde (ör. `assets`/`timeline` şemasının kırıcı değişimi) yükleyicinin yanlış yorumlama yapmasını engeller.

## Alternatifler

- **Alternatif A — Sürümü `0.1.0`'da bırak, yalnızca `#[serde(default)]` alan ekle:** En düşük riskli görünür, ancak spec'in `0.2.0` minor genişleme tanımıyla çelişir, yeni yetenekleri sürüm izi olmadan biriktirir ve "hangi proje hangi alanları bekliyor" bilgisini kaybeder. Reddedildi.
- **Alternatif B — Katı major eşitliği, her minor'u kabul et (minor kontrolünü tamamen kaldır):** İleri uyumsuzluğu gizler; yeni minor'daki bilinmeyen alanlar sessizce düşer veya yanlış yorumlanır. Veri kaybı riski nedeniyle reddedildi.
- **Alternatif C — Yalnızca patch artışı (`0.1.1`):** Teknik olarak yüklenir, ama patch SemVer'de hata düzeltmesidir; yeni opsiyonel alanlar (fade, burst, scrim, reframe, ribbon) için doğru sürüm sinyali değildir. Reddedildi.
- **Alternatif D — Çift sürüm okuma + açık migrasyon katmanı:** Geriye dönük en güvenli yol olurdu, ama her yeni minor için migratör bakımı gerektirir ve mevcut serde varsayılanı altyapısı varken gereksiz karmaşıklık ekler. Alan semantiği kırıcı biçimde değişirse (major bump) yeniden değerlendirilebilir.

## Sonuçlar ve Kısıtlar

- **Etkilenen alanlar:** `crates/frameshift-schema/src/version.rs` (`CURRENT_SCHEMA_VERSION`, `is_compatible_with_current`), `crates/frameshift-schema/tests/schema_tests.rs` (sürüm uyumluluk testleri), [project-schema.md](../specs/project-schema.md) (sürüm maddesi, hata davranışı, kabul kriterleri). `Project::validate` çağrı noktası değişmez; kural aynı fonksiyon üzerinden uygulanır.
- **Doğrulanması gerekenler:**
  - `0.1.0` (gerçek `local_media` fixture'ı dahil), `0.2.0` yüklenmeli; `0.3.0` ve `1.0.0` reddedilmeli; bozuk sürüm metni reddedilmeli.
  - Patch bileşeninin uyumluluğu etkilemediği testle sabitlenmeli (`0.1.9`, `0.2.7` kabul).
  - Gelecekteki minor artışlarında yeni alanların `#[serde(default)]` taşıması gözden geçirilmelidir.
- **Geri alınamazlık:** Bu kural gevşemesi eski projeleri yüklenebilir kılar; ancak `0.2.0` ile yazılmış bir proje eski (`0.1.0`) ikili tarafından okunamaz. Bu beklenen ileri uyumsuzluktur ve kullanıcıya açık hata olarak görünür.
