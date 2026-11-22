# 2026-11-21 — Detaylı Implementasyon Planı Hazırlığı

**Tarih:** 2026-11-21  
**Faz:** Faz 0 — Fikir, Tasarım ve Mimari Sözleşme Hazırlığı (Kod Öncesi)  
**Durum:** Tamamlandı (Kullanıcı onayı bekleniyor)

---

## Amaç

FrameShift için onaylanan mimari yön (`Rust + wgpu/WGSL + FFmpeg + OpenColorIO + Text/Vector`, bağımsız çekirdek + masaüstü UI + headless desteği) doğrultusunda Faz 0'dan Faz 5'e kadar risk-öncelikli, dikey dilim tabanlı detaylı implementasyon planı hazırlamak.

---

## Yapılanlar

1. Oturum başı okuma rotası tamamlandı (`brain/state.md`, `brain/index.md`, `brain/lessons.md`, ADR 0001–0003, `brain/specs/README.md`, `PROJECT_NOTES.md`).
2. Risk-First Dikey Dilim (Vertical Slice) yaklaşımına dayalı kapsamlı implementasyon planı hazırlandı ve repo köküne [IMPLEMENTATION_PLAN.md](../../IMPLEMENTATION_PLAN.md) olarak kaydedildi:
   - 9 crate'lik Rust monorepo/workspace mimarisi (`frameshift-time`, `frameshift-schema`, `frameshift-media`, `frameshift-render`, `frameshift-vector`, `frameshift-core`, `frameshift-cli`, `frameshift-mcp`, `frameshift-app`).
   - Faz 0 (Sözleşmelerin Kilitlenmesi) → Faz 1/M1 (Golden Vertical Slice) → Faz 2/M2 (Anlamsal Zaman, İnceleme, Vektör & Renk) → Faz 3/M3 (Agent & MCP) → Faz 4/M4 (Masaüstü UI) → Faz 5/M5 (HW Zero-Copy, Proxy & Dağıtım) yol haritası ve çıkış kapıları.
   - Kritik mühendislik riskleri ve kör nokta azaltma matrisi (FFmpeg Windows/LGPL build, Skia vs Vello, OCIO FFI ağırlığı, GPU TDR, VFR drift, preview/final ayrışması).
   - Kullanıcının karar yükünü azaltan 3 somut başlangıç tercihi.
3. Kodlama, scaffold veya bağımlılık kurulumu başlatılmadı (`AGENTS.md` sınırına uyuldu).

---

## Doğrulama ve Kanıtlar

- Planın tüm maddeleri mevcut `AGENTS.md`, `PROJECT_NOTES.md` ve ADR 0001–0003 kararlarıyla tutarlılık açısından kontrol edildi.
- Epistemik sınır korundu: henüz hiçbir kod, crate veya render çıktısı olmadığı açıkça belirtildi.

---

## Açık Kalanlar / Sıradaki Adım

- `/grill-me` oturumuyla 6 temel başlangıç kararı kullanıcı tarafından onaylandı (Hibrit Hızlı Yol, Flicks + VFR PTS indeksleme, Windows birincil + LGPL FFmpeg yazılım decode → GPU upload, M1 hafif renk/alfa + M2 OCIO/`vello` vs `skia` PoC, çift katmanlı test medyası, Rust-Native UI yönelimi).
- Sıradaki adım: `brain/specs/timebase.md` ve minimal `brain/specs/project-schema.md` sözleşmelerini yazıp [ADR-0002](../decisions/0002-rational-timebase-flicks.md) kararını kullanıcı onayıyla `accepted` durumuna getirmek.
