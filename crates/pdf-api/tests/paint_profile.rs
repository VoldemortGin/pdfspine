use pdf_api::{page_get_paint_profile, Document, PaintDisposition, ResourceOrigin};

fn build_pdf(objects: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = std::collections::HashMap::new();
    let mut max_num = 0;
    for (number, body) in objects {
        offsets.insert(*number, out.len());
        out.extend_from_slice(format!("{number} 0 obj\n").as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
        max_num = max_num.max(*number);
    }
    let size = max_num + 1;
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {size}\n").as_bytes());
    out.extend_from_slice(b"0000000000 65535 f \n");
    for number in 1..size {
        if let Some(offset) = offsets.get(&number) {
            out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        } else {
            out.extend_from_slice(b"0000000000 65535 f \n");
        }
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {size} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    out
}

fn stream(body: &[u8], extra: &str) -> Vec<u8> {
    let mut out = format!("<< /Length {} {extra} >>\nstream\n", body.len()).into_bytes();
    out.extend_from_slice(body);
    out.extend_from_slice(b"\nendstream");
    out
}

fn one_page(content: &[u8], page_resources: &str, inherited_resources: &str) -> Vec<u8> {
    build_pdf(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            format!(
                "<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] {inherited_resources} >>"
            )
            .into_bytes(),
        ),
        (
            3,
            format!(
                "<< /Type /Page /Parent 2 0 R /Contents 4 0 R {page_resources} >>"
            )
            .into_bytes(),
        ),
        (4, stream(content, "")),
    ])
}

#[test]
fn inherited_resources_are_shared_with_the_strict_profile() {
    let bytes = one_page(
        b"/GS1 gs 1 j 10 M 1 w 10 10 20 20 re S",
        "",
        "/Resources << /ExtGState << /GS1 << /Type /ExtGState /BM /Normal /CA 1 /ca 1 >> >> >>",
    );
    let doc = Document::open_bytes(bytes).unwrap();
    let page = doc.load_page(0).unwrap();
    let profile = page_get_paint_profile(&page);

    assert!(profile.complete, "{:?}", profile.diagnostics);
    assert_eq!(profile.resource_scopes[0].origin, ResourceOrigin::Inherited);
    let gs = profile.resource_scopes[0]
        .entries
        .iter()
        .find(|entry| entry.name == "GS1")
        .unwrap();
    assert!(gs.selected);
    assert_eq!(gs.ext_gstate.as_ref().unwrap().stroke_alpha, Some(1.0));
    assert!(profile
        .operators
        .iter()
        .any(|operator| { operator.mnemonic == "j" && operator.numeric_operands == [1.0] }));
}

#[test]
fn unsupported_operator_and_inline_image_cannot_report_complete() {
    let bytes = one_page(
        b"1 2 XX /RelativeColorimetric ri 1 i 0 0 d0 BI /W 1 /H 1 ID x EI",
        "/Resources << >>",
        "",
    );
    let doc = Document::open_bytes(bytes).unwrap();
    let profile = page_get_paint_profile(&doc.load_page(0).unwrap());

    assert!(!profile.complete);
    assert!(profile.operators.iter().any(|operator| {
        operator.mnemonic == "XX" && operator.disposition == PaintDisposition::Unsupported
    }));
    assert_eq!(profile.inline_images.len(), 1);
    for mnemonic in ["ri", "i", "d0"] {
        assert!(profile.operators.iter().any(|operator| {
            operator.mnemonic == mnemonic && operator.disposition == PaintDisposition::Unsupported
        }));
    }
    assert!(profile
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "inline_image_not_strictly_accounted"));
}

#[test]
fn selected_pattern_is_typed_and_fails_closed() {
    let bytes = one_page(
        b"/P1 scn 0 0 1 1 re f",
        "/Resources << /Pattern << /P1 << /PatternType 1 >> >> >>",
        "",
    );
    let doc = Document::open_bytes(bytes).unwrap();
    let profile = page_get_paint_profile(&doc.load_page(0).unwrap());

    assert!(!profile.complete);
    let pattern = profile.resource_scopes[0]
        .entries
        .iter()
        .find(|entry| entry.category == "Pattern" && entry.name == "P1")
        .unwrap();
    assert!(pattern.selected);
    assert!(profile
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "pattern_not_strictly_accounted"));
}

#[test]
fn selected_unknown_extgstate_and_soft_mask_fail_closed() {
    let bytes = one_page(
        b"/GS1 gs 10 10 20 20 re f",
        "/Resources << /ExtGState << /GS1 << /Type /ExtGState /BM /Multiply /SMask 9 0 R /OP true >> >> >>",
        "",
    );
    let doc = Document::open_bytes(bytes).unwrap();
    let profile = page_get_paint_profile(&doc.load_page(0).unwrap());

    assert!(!profile.complete);
    assert!(profile
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "extgstate_semantics_unsupported"));
}

#[test]
fn bounded_page_group_is_typed_but_other_group_semantics_fail_closed() {
    fn with_group(group: &str) -> Vec<u8> {
        build_pdf(&[
            (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
            (
                2,
                b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>"
                    .to_vec(),
            ),
            (
                3,
                format!(
                    "<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << >> /Group {group} >>"
                )
                .into_bytes(),
            ),
            (4, stream(b"0 0 10 10 re f", "")),
        ])
    }

    let doc = Document::open_bytes(with_group(
        "<< /Type /Group /S /Transparency /CS /DeviceRGB >>",
    ))
    .unwrap();
    let profile = page_get_paint_profile(&doc.load_page(0).unwrap());
    assert!(profile.complete, "{:?}", profile.diagnostics);
    let group = profile.resource_scopes[0].group.as_ref().unwrap();
    assert_eq!(group.group_type.as_deref(), Some("Group"));
    assert_eq!(group.subtype.as_deref(), Some("Transparency"));
    assert_eq!(group.color_space.as_deref(), Some("DeviceRGB"));
    assert_eq!(group.isolated, None);
    assert_eq!(group.knockout, None);

    for group in [
        "<< /Type /Group /S /Transparency /CS /DeviceGray >>",
        "<< /Type /Group /S /Transparency /CS /DeviceRGB /I true >>",
        "<< /Type /Group /S /Transparency /CS /DeviceRGB /K true >>",
        "<< /Type /Group /S /Transparency /CS /DeviceRGB /Unknown 1 >>",
    ] {
        let doc = Document::open_bytes(with_group(group)).unwrap();
        let profile = page_get_paint_profile(&doc.load_page(0).unwrap());
        assert!(!profile.complete, "{group}");
        assert!(profile
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "transparency_group_unsupported"));
    }
}

#[test]
fn fractional_selected_alpha_and_text_clipping_fail_closed() {
    let alpha = one_page(
        b"/GS1 gs 0 0 10 10 re f",
        "/Resources << /ExtGState << /GS1 << /Type /ExtGState /BM /Normal /ca 0.999 >> >> >>",
        "",
    );
    let doc = Document::open_bytes(alpha).unwrap();
    let profile = page_get_paint_profile(&doc.load_page(0).unwrap());
    assert!(!profile.complete);
    assert!(profile.resource_scopes[0].entries.iter().any(|entry| {
        entry.ext_gstate.as_ref().is_some_and(|state| {
            state
                .unsupported_keys
                .iter()
                .any(|key| key == "non_binary:ca")
        })
    }));

    for mode in 4..=7 {
        let content = format!("BT {mode} Tr ET");
        let doc =
            Document::open_bytes(one_page(content.as_bytes(), "/Resources << >>", "")).unwrap();
        let profile = page_get_paint_profile(&doc.load_page(0).unwrap());
        assert!(!profile.complete, "Tr={mode}");
        assert!(profile
            .diagnostics
            .iter()
            .any(|diagnostic| { diagnostic.code == "text_clipping_not_strictly_accounted" }));
    }
}

#[test]
fn form_parent_resources_are_explicit_and_transparency_group_rejects() {
    let content = b"/Fm1 Do";
    let form = stream(
        b"/GS1 gs 0 0 10 10 re f",
        "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Group << /S /Transparency /I true >>",
    );
    let bytes = build_pdf(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Fm1 5 0 R >> /ExtGState << /GS1 << /Type /ExtGState /BM /Normal >> >> >> >>"
                .to_vec(),
        ),
        (4, stream(content, "")),
        (5, form),
    ]);
    let doc = Document::open_bytes(bytes).unwrap();
    let profile = page_get_paint_profile(&doc.load_page(0).unwrap());

    assert!(!profile.complete);
    assert_eq!(profile.resource_scopes.len(), 2);
    assert_eq!(
        profile.resource_scopes[1].origin,
        ResourceOrigin::ParentFallback
    );
    assert!(profile.resource_scopes[1]
        .group_keys
        .iter()
        .any(|key| key == "S"));
    assert!(profile
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "transparency_group_unsupported"));
}

#[test]
fn form_owned_resources_are_distinct_and_can_be_complete() {
    let form = stream(
        b"/GS1 gs 0 0 10 10 re f",
        "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /ExtGState << /GS1 << /Type /ExtGState /BM /Normal >> >> >>",
    );
    let bytes = build_pdf(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /Fm1 5 0 R >> >> >>"
                .to_vec(),
        ),
        (4, stream(b"/Fm1 Do", "")),
        (5, form),
    ]);
    let doc = Document::open_bytes(bytes).unwrap();
    let profile = page_get_paint_profile(&doc.load_page(0).unwrap());

    assert!(profile.complete, "{:?}", profile.diagnostics);
    assert_eq!(profile.resource_scopes.len(), 2);
    assert_eq!(profile.resource_scopes[1].origin, ResourceOrigin::Direct);
    assert!(profile.resource_scopes[1]
        .entries
        .iter()
        .any(|entry| entry.category == "ExtGState" && entry.name == "GS1" && entry.selected));
}

#[test]
fn broken_contents_and_page_parent_cycle_are_explicit() {
    let broken_contents = build_pdf(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /Contents 99 0 R /Resources << >> >>".to_vec(),
        ),
    ]);
    let doc = Document::open_bytes(broken_contents).unwrap();
    let profile = page_get_paint_profile(&doc.load_page(0).unwrap());
    assert!(!profile.complete);
    assert!(profile
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "content_reference_unresolved"));

    let parent_cycle = build_pdf(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 3 0 R /Contents 4 0 R >>".to_vec(),
        ),
        (4, stream(b"0 0 1 1 re f", "")),
    ]);
    let doc = Document::open_bytes(parent_cycle).unwrap();
    let profile = page_get_paint_profile(&doc.load_page(0).unwrap());
    assert!(!profile.complete);
    assert!(profile
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "page_resource_parent_cycle"));
}

#[test]
fn malformed_inline_image_and_broken_page_resource_reference_are_explicit() {
    let bytes = build_pdf(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources 99 0 R >>".to_vec(),
        ),
        (4, stream(b"BI /W 1 /H 1 ID unterminated", "")),
    ]);
    let doc = Document::open_bytes(bytes).unwrap();
    let profile = page_get_paint_profile(&doc.load_page(0).unwrap());

    assert!(!profile.complete);
    let codes: Vec<_> = profile
        .diagnostics
        .iter()
        .map(|diagnostic| diagnostic.code)
        .collect();
    assert!(codes.contains(&"page_resource_reference_unresolved"));
    assert!(codes.contains(&"inline_image_missing_ei"));
}

#[test]
fn form_cycle_and_depth_limit_are_not_silent() {
    let cycle_form = stream(
        b"/Self Do",
        "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /XObject << /Self 5 0 R >> >>",
    );
    let cycle_pdf = build_pdf(&[
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /F 5 0 R >> >> >>"
                .to_vec(),
        ),
        (4, stream(b"/F Do", "")),
        (5, cycle_form),
    ]);
    let doc = Document::open_bytes(cycle_pdf).unwrap();
    let cycle = page_get_paint_profile(&doc.load_page(0).unwrap());
    assert!(cycle
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "form_cycle"));

    let mut objects = vec![
        (1, b"<< /Type /Catalog /Pages 2 0 R >>".to_vec()),
        (
            2,
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 100 100] >>".to_vec(),
        ),
        (
            3,
            b"<< /Type /Page /Parent 2 0 R /Contents 4 0 R /Resources << /XObject << /F 5 0 R >> >> >>"
                .to_vec(),
        ),
        (4, stream(b"/F Do", "")),
    ];
    for number in 5..=21 {
        let next = number + 1;
        objects.push((
            number,
            stream(
                format!("/F Do % {number}").as_bytes(),
                &format!(
                    "/Type /XObject /Subtype /Form /BBox [0 0 10 10] /Resources << /XObject << /F {next} 0 R >> >>"
                ),
            ),
        ));
    }
    objects.push((
        22,
        stream(
            b"0 0 1 1 re f",
            "/Type /XObject /Subtype /Form /BBox [0 0 10 10]",
        ),
    ));
    let doc = Document::open_bytes(build_pdf(&objects)).unwrap();
    let depth = page_get_paint_profile(&doc.load_page(0).unwrap());
    assert!(depth
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.code == "form_depth_exceeded"));
}

#[test]
fn invalid_operands_optional_content_and_missing_colorspace_fail_closed() {
    for (content, expected) in [
        (&b"1 0 cm"[..], "invalid_operator_operands"),
        (&b"Q"[..], "graphics_state_stack_underflow"),
        (
            &b"/OC /Layer1 BDC 0 0 1 1 re f EMC"[..],
            "optional_content_not_strictly_accounted",
        ),
        (
            &b"/Missing cs 0.5 scn 0 0 1 1 re f"[..],
            "colorspace_dictionary_unresolved",
        ),
        (
            &b"BT /Missing 12 Tf (72%) Tj ET"[..],
            "font_dictionary_unresolved",
        ),
        (&b"BT [/Bad (72%)] TJ ET"[..], "invalid_operator_operands"),
    ] {
        let bytes = one_page(content, "/Resources << >>", "");
        let doc = Document::open_bytes(bytes).unwrap();
        let profile = page_get_paint_profile(&doc.load_page(0).unwrap());
        assert!(!profile.complete, "{content:?}");
        assert!(
            profile
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == expected),
            "{content:?}: {:?}",
            profile.diagnostics
        );
    }
}

#[test]
fn selected_font_and_solid_dash_are_valid_typed_inputs() {
    let bytes = one_page(
        b"[] 0 d BT /F1 12 Tf (72%) Tj ET",
        "/Resources << /Font << /F1 << /Type /Font /Subtype /Type1 /BaseFont /Helvetica >> >> >>",
        "",
    );
    let doc = Document::open_bytes(bytes).unwrap();
    let profile = page_get_paint_profile(&doc.load_page(0).unwrap());

    assert!(profile.complete, "{:?}", profile.diagnostics);
    assert!(profile.resource_scopes[0]
        .entries
        .iter()
        .any(|entry| entry.category == "Font" && entry.name == "F1" && entry.selected));
}

#[test]
fn unterminated_compound_operands_are_diagnostics() {
    for (content, expected) in [
        (&b"[1 2"[..], "unterminated_array"),
        (&b"<< /A 1"[..], "unterminated_dictionary"),
        (&b"<< /A >>"[..], "missing_dictionary_value"),
    ] {
        let bytes = one_page(content, "/Resources << >>", "");
        let doc = Document::open_bytes(bytes).unwrap();
        let profile = page_get_paint_profile(&doc.load_page(0).unwrap());
        assert!(!profile.complete);
        assert!(
            profile
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == expected),
            "{content:?}: {:?}",
            profile.diagnostics
        );
    }
}
