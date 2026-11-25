# 2026-11-25 — OpenDesign Motion Kalıpları & Stil Rehberinin Eklenmesi

## Amaç
[`nexu-io/open-design`](https://github.com/nexu-io/open-design) (Apache-2.0) deposundaki yeni OpenMotion galerisi ve HyperFrames motion dokümantasyonundan FrameShift ajanlarının işine yarayacak, motor sınırlarına uygun kısımları ayıklayıp kalıcı belleğe eklemek.

## Yapılanlar
1. **Araştırma & Rehber Belgesi:** [brain/research/open-design-motion-patterns.md](../research/open-design-motion-patterns.md) oluşturuldu:
   - `motion-principles.md`, `data-in-motion.md` ve `house-style.md` üzerinden ajan motion koruma kuralları (`Build/Breathe/Resolve`, easing yön/hız çeşitliliği, kompozisyon katmanları) derlendi.
   - `visual-styles.md` içindeki 8 görsel stil arketipi (*Swiss Pulse*, *Velvet Standard*, *Deconstructed*, *Maximalist Type*, *Data Drift*, *Soft Signal*, *Folk Frequency*, *Shadow Cut*) FrameShift'in mevcut `NodeKind`, `SafeExpr`, WGSL geçişleri ve `CubicBezierSpeed` yeteneklerine eşlendi.
   - `docs/motion-templates/catalog.json` (286 referans) özetlendi.
   - HTML/CSS/GSAP render katmanı `AGENTS.md` (web/DOM katmanlı yaklaşımlara dönmeme) kuralı gereği hariç tutuldu.
2. **Bellek Güncellemeleri:** [brain/index.md](../index.md) ve [brain/state.md](../state.md) güncellendi.

## Doğrulama
- Yalnızca dokümantasyon/`brain/` dosyaları eklendi/güncellendi; Rust workspace kodunda değişiklik yapılmadı (`git status` ile doğrulanacak).
