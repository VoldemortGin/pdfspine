//! `MD-DET-*` — determinism: identical input ⇒ identical bytes (no timestamps,
//! no randomness, ordered maps, fixed object-allocation order).

mod common;

use pdf_markdown::{markdown_to_pdf, Options};

const SANS: &[u8] = include_bytes!("../../pdf-fonts/fonts/liberation/LiberationSans-Regular.ttf");

/// A 1×1 red PNG as a data URI (fixed bytes → fully self-contained input).
const PNG_URI: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAIAAACQd1PeAAAADElEQVR4nGP4z8AAAAMBAQDJ/pLvAAAAAElFTkSuQmCC";

fn kitchen_sink() -> String {
    format!(
        "# Title\n\n\
         Paragraph with **bold**, *italic*, `code`, ~~strike~~ and a \
         [link](https://example.com).\n\n\
         > A quote\n> spanning lines\n\n\
         - one\n- two\n  - nested\n\n\
         1. first\n2. second\n\n\
         - [x] done\n- [ ] todo\n\n\
         ```\nlet x = 42;\n```\n\n\
         | A | B |\n| - | - |\n| a1 | b1 |\n\n\
         ---\n\n\
         ![dot]({PNG_URI})\n\n\
         Final paragraph. 你好世界.\n"
    )
}

#[test]
fn same_input_same_bytes_default_options() {
    let md = kitchen_sink();
    let a = markdown_to_pdf(&md, &Options::default()).expect("render a");
    let b = markdown_to_pdf(&md, &Options::default()).expect("render b");
    assert_eq!(a, b, "two renders of the same input must be byte-identical");
}

#[test]
fn same_input_same_bytes_with_embedded_fonts() {
    let md = kitchen_sink();
    let mut opts = Options::default();
    opts.font = Some(SANS.to_vec());
    opts.cjk_font = Some(SANS.to_vec());
    let a = markdown_to_pdf(&md, &opts).expect("render a");
    let b = markdown_to_pdf(&md, &opts).expect("render b");
    assert_eq!(a, b, "embedded-font renders must be byte-identical");
}

#[test]
fn different_input_different_bytes() {
    let a = markdown_to_pdf("alpha", &Options::default()).expect("render a");
    let b = markdown_to_pdf("beta", &Options::default()).expect("render b");
    assert_ne!(a, b);
}
