use std::{collections::BTreeMap, fs};
use xtask::skills::{validate_content, write_archive};

#[test]
fn skill_format_conformance() {
    assert!(
        validate_content(
            "---\nname: metadata\ndescription: Useful\nmetadata:\n  01: first\n  1: second\n---",
            "metadata"
        )
        .is_ok()
    );
    assert!(
        validate_content(
            "---\nname: a\u{0345}\ndescription: Useful\n---",
            "a\u{0345}"
        )
        .is_err()
    );
    assert!(
        validate_content(
            "---\nname: 001\ndescription: true\ncompatibility:\n---",
            "001"
        )
        .is_ok()
    );
    assert!(
        validate_content(
            "---\nname: duplicate\nname: duplicate\ndescription: Useful\n---",
            "duplicate"
        )
        .is_err()
    );
    for (name, content, valid) in [
        (
            "valid-skill",
            "---\nname: valid-skill\ndescription: Useful skill\n---\nBody",
            true,
        ),
        (
            "日本語",
            "---\nname: 日本語\ndescription: Useful\nmetadata:\n  author: someone\n---",
            true,
        ),
        (
            "fullwidth",
            "---\nname: ｆｕｌｌｗｉｄｔｈ\ndescription: Useful\n---",
            true,
        ),
        ("missing", "name: missing\ndescription: Useful", false),
        ("missing", "---\nname: missing\ndescription: Useful", false),
        ("missing", "---\nname: missing\n---", false),
        ("wrong", "---\nname: other\ndescription: Useful\n---", false),
        (
            "Capital",
            "---\nname: Capital\ndescription: Useful\n---",
            false,
        ),
        (
            "two--hyphens",
            "---\nname: two--hyphens\ndescription: Useful\n---",
            false,
        ),
        ("-edge", "---\nname: -edge\ndescription: Useful\n---", false),
        (
            "unknown",
            "---\nname: unknown\ndescription: Useful\nextra: forbidden\n---",
            false,
        ),
        ("empty", "---\nname: empty\ndescription: '   '\n---", false),
        (
            "flow",
            "---\nname: flow\ndescription: Useful\nmetadata: {author: someone}\n---",
            false,
        ),
        (
            "alias",
            "---\nname: &name alias\ndescription: *name\n---",
            false,
        ),
        ("list", "---\n- name\n- description\n---", false),
    ] {
        assert_eq!(validate_content(content, name).is_ok(), valid, "{content}");
    }
    for (field, limit) in [("description", 1024), ("compatibility", 500)] {
        let base = if field == "description" {
            "---\nname: length\n"
        } else {
            "---\nname: length\ndescription: Useful\n"
        };
        assert!(
            validate_content(
                &format!("{base}{field}: {}\n---", "é".repeat(limit)),
                "length"
            )
            .is_ok()
        );
        assert!(
            validate_content(
                &format!("{base}{field}: {}\n---", "é".repeat(limit + 1)),
                "length"
            )
            .is_err()
        );
    }
}

#[test]
fn archive_bytes_are_reproducible_and_extraction_refuses_links() {
    let temp = tempfile::tempdir().unwrap();
    let files = BTreeMap::from([
        ("a.txt".into(), b"a".to_vec()),
        ("sub/b.txt".into(), b"b".to_vec()),
    ]);
    let first = temp.path().join("first.tar.gz");
    let second = temp.path().join("second.tar.gz");
    write_archive(&first, "kit", &files).unwrap();
    write_archive(&second, "kit", &files).unwrap();
    assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());
    let extracted = xtask::skills::extract(&first, &temp.path().join("extracted")).unwrap();
    assert_eq!(xtask::files(&extracted).unwrap(), files);
    let malicious = temp.path().join("link.tar.gz");
    let gzip = flate2::write::GzEncoder::new(
        fs::File::create(&malicious).unwrap(),
        flate2::Compression::default(),
    );
    let mut tar = tar::Builder::new(gzip);
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o777);
    header.set_cksum();
    tar.append_link(&mut header, "kit/link", "../../outside")
        .unwrap();
    tar.into_inner().unwrap().finish().unwrap();
    assert!(xtask::skills::extract(&malicious, &temp.path().join("bad")).is_err());
}
