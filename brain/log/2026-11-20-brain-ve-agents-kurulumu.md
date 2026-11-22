# 2026-11-20 — `brain/` ve `AGENTS.md` Kurulumu

**Tarih:** 2026-11-20  
**Faz:** Faz 0 — Tasarım ve Mimari Sözleşme Hazırlığı (Kod Öncesi)  
**Durum:** Tamamlandı

---

## Amaç

FrameShift projesinde çalışan tüm ajanların mimari kararları, epistemik sınırları, açık soruları ve geçmiş dersleri oturumlar arasında kaybetmeden çalışabilmesi için kalıcı bellek (`brain/`) ve bağlayıcı ajan talimatları (`AGENTS.md` / `CLAUDE.md`) sistemini kurmak.

---

## Yapılanlar

1. Kök dizinde bağlayıcı kural kaynağı olarak [AGENTS.md](../../AGENTS.md) ve ince yönlendirme dosyası olarak [CLAUDE.md](../../CLAUDE.md) oluşturuldu.
2. `brain/` altında 3 katmanlı kalıcı hafıza yapısı kuruldu:
   - [brain/README.md](../README.md), [brain/index.md](../index.md), [brain/state.md](../state.md), [brain/lessons.md](../lessons.md)
   - İlk 3 ADR: [0001](../decisions/0001-independent-native-gpu-core.md) (`accepted`), [0002](../decisions/0002-rational-timebase-flicks.md) (`proposed`), [0003](../decisions/0003-semantic-event-constraint-timing.md) (`accepted`)
   - Çekirdek sözleşme alanı [brain/specs/README.md](../specs/README.md) ve araştırma köprüsü [brain/research/README.md](../research/README.md)
   - Gelecek kayıtlar için `_templates/` altında `karar.md`, `spec.md`, `arastirma.md` ve `gunluk.md` şablonları eklendi.
3. Mevcut 4 kök doküman (`README.md`, `PROJECT_NOTES.md`, `ENGINE_ALTERNATIVES_RESEARCH.md`, `VISUAL_REFERENCES.md`) değiştirilmeden korundu.

---

## Doğrulama ve Kanıtlar

- `AGENTS.md` satır sayısı 140 satır sınırının altında tutuldu.
- `git status --short` ile yalnızca hedeflenen yeni dosyaların eklendiği ve mevcut kök dokümanlara dokunulmadığı teyit edildi.
- Tüm içerikler %100 FrameShift gerçekliğine (kod öncesi tasarım aşaması) göre doğrulandı.

---

## Sıradaki Adım

- ADR `0002` içindeki açık noktaları netleştirerek `brain/specs/timebase.md` taslağını hazırlamak ve kullanıcı onayına sunmak.
