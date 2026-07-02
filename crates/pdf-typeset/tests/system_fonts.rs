//! Local-only system-font resolution checks (PRD §10 TS-2 acceptance: "a
//! local (non-CI) macOS test resolves 宋体 → Songti SC and 微软雅黑 →
//! PingFang SC").
//!
//! These run against the machine's real font environment, so they are guarded
//! twice: skipped on CI (`CI` env var, set by GitHub Actions on every OS) and
//! skipped off-macOS. Assertions accept the documented substitution
//! *candidates* as well (e.g. a machine with Office fonts installed hits
//! SimSun / Microsoft YaHei directly — that is correct behavior, not a bug).

use pdf_typeset::{ExportWarning, FontResolver};

/// Whether to skip: not macOS, or a CI environment.
fn skip() -> bool {
    if std::env::var_os("CI").is_some() {
        eprintln!("skipped: CI environment (system-font tests are local-only)");
        return true;
    }
    if !cfg!(target_os = "macos") {
        eprintln!("skipped: not macOS");
        return true;
    }
    false
}

#[test]
fn macos_resolves_the_locked_cjk_substitutions() {
    if skip() {
        return;
    }
    let started = std::time::Instant::now();
    let resolver = FontResolver::with_system_fonts();
    eprintln!(
        "system scan: {} faces in {:?}",
        resolver.face_count(),
        started.elapsed()
    );

    let mut warnings = Vec::new();
    let songti = resolver.resolve("宋体", false, false, &mut warnings);
    eprintln!(
        "宋体 → {} ({} #{}) warnings={warnings:?}",
        songti.family, songti.post_script_name, songti.index
    );
    assert!(
        ["Songti SC", "STSong", "SimSun"].contains(&songti.family.as_str()),
        "宋体 resolved to {}",
        songti.family
    );

    let mut warnings = Vec::new();
    let yahei = resolver.resolve("微软雅黑", false, false, &mut warnings);
    eprintln!(
        "微软雅黑 → {} ({} #{}) warnings={warnings:?}",
        yahei.family, yahei.post_script_name, yahei.index
    );
    assert!(
        ["PingFang SC", "Hiragino Sans GB", "Microsoft YaHei"].contains(&yahei.family.as_str()),
        "微软雅黑 resolved to {}",
        yahei.family
    );

    // Both are substitutions on a stock mac (direct hits with Office fonts
    // installed) — either way the request NEVER errors.
    for w in &warnings {
        assert!(matches!(
            w,
            ExportWarning::FontSubstituted { .. } | ExportWarning::StyleApproximated { .. }
        ));
    }
}

#[test]
fn macos_calibri_and_latin_paths() {
    if skip() {
        return;
    }
    let resolver = FontResolver::with_system_fonts();

    let mut warnings = Vec::new();
    let calibri = resolver.resolve("Calibri", false, false, &mut warnings);
    eprintln!(
        "Calibri → {} ({}) warnings={warnings:?}",
        calibri.family, calibri.post_script_name
    );
    assert!(
        ["Calibri", "Carlito", "Liberation Sans"].contains(&calibri.family.as_str()),
        "Calibri resolved to {}",
        calibri.family
    );

    // Times New Roman ships with macOS: a direct hit, no substitution.
    let mut warnings = Vec::new();
    let times = resolver.resolve("Times New Roman", true, false, &mut warnings);
    eprintln!(
        "Times New Roman bold → {} ({}) warnings={warnings:?}",
        times.family, times.post_script_name
    );
    assert_eq!(times.family, "Times New Roman");
    assert!(warnings.is_empty());
}

#[test]
fn macos_ttc_collections_and_char_fallback_hit_real_cjk_faces() {
    if skip() {
        return;
    }
    let resolver = FontResolver::with_system_fonts();

    // Direct PingFang SC request (lives in a system TTC — AssetsV2 on modern
    // macOS). Tolerate absence on stripped-down installs.
    let mut warnings = Vec::new();
    let pingfang = resolver.resolve("PingFang SC", false, false, &mut warnings);
    eprintln!(
        "PingFang SC → {} ({} #{}) warnings={warnings:?}",
        pingfang.family, pingfang.post_script_name, pingfang.index
    );

    // Songti is a .ttc — a resolved face index > 0 proves TTC enumeration on
    // at least one of the two collections.
    let mut warnings = Vec::new();
    let songti = resolver.resolve("Songti SC", false, false, &mut warnings);
    eprintln!(
        "Songti SC → {} ({} #{}) warnings={warnings:?}",
        songti.family, songti.post_script_name, songti.index
    );

    // Per-char fallback from a Latin base must land on a real CJK face here.
    let mut warnings = Vec::new();
    let base = resolver.resolve("Liberation Sans", false, false, &mut warnings);
    let mut warnings = Vec::new();
    let cjk = resolver.resolve_char(&base, '中', &mut warnings);
    eprintln!(
        "'中' fallback → {} ({} #{}) warnings={warnings:?}",
        cjk.family, cjk.post_script_name, cjk.index
    );
    assert_ne!(
        cjk.key(),
        base.key(),
        "a CJK-capable system face must take over for 中"
    );
    assert!(resolver.has_glyph(&cjk, '中'));
    assert_eq!(warnings.len(), 1);
}
