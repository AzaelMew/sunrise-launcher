use super::*;

const REPO: &str = "owner/project";
const SHA: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn markdown_strips_html_and_only_opens_safe_https_links_from_the_pinned_directory() {
    let readme = Saved {
        path: "docs/README.md".into(),
        markdown: "# Atlas\n\n<i>Native text</i> and [`next`](guide.md#install).\n\n<script>nope()</script>\n".into(),
    }.parse(REPO, SHA, true);
    assert!(readme.markdown().contains("Native text"));
    assert!(!readme.markdown().contains("<i>"));
    assert!(!readme.markdown().contains("nope()"));
    assert_eq!(
        readme.link("guide.md#install"),
        Some(format!(
            "https://github.com/{REPO}/blob/{SHA}/docs/guide.md#install"
        ))
    );
    assert_eq!(
        readme.link("../LICENSE"),
        Some(format!("https://github.com/{REPO}/blob/{SHA}/LICENSE"))
    );
    assert_eq!(
        readme.link("https://example.test/"),
        Some("https://example.test/".into())
    );
    for refused in [
        "javascript:alert(1)",
        "http://example.test/",
        "https://user@example.test/a",
        "https://example.test:444/",
        "//example.test/a",
        "..\\LICENSE",
    ] {
        assert!(readme.link(refused).is_none(), "{refused}");
    }
}

#[test]
fn only_eight_same_repository_pinned_images_can_be_loaded() {
    let mut markdown = [
        "![not here](https://example.test/track.png)",
        "![not here](//raw.githubusercontent.com/owner/project/main/camps.png)",
        "![not here](https://raw.githubusercontent.com/owner/project/main/camps.png)",
        "![not here](../camps.png)",
        "![not here](media/%2e%2e/camps.png)",
        "![not here](https://user@raw.githubusercontent.com/owner/project/main/camps.png)",
        "![First **camp**](media/first.png)",
    ]
    .join("\n\n");
    markdown.push_str(&format!(
        "\n\n![second](https://raw.githubusercontent.com/{REPO}/{SHA}/other.jpg)"
    ));
    for n in 0..10 {
        markdown.push_str(&format!("\n\n![more](/media/{n}.webp)"));
    }
    let readme = Saved {
        path: "docs/README.md".into(),
        markdown,
    }
    .parse(REPO, SHA, false);
    let images = readme.images();
    assert_eq!(images.len(), 8);
    assert_eq!(images[0].alt(), "First camp");
    assert_eq!(images[0].destination(), "media/first.png");
    assert_eq!(images[0].location, repo("docs/media/first.png"));
    assert_eq!(images[1].location, repo("other.jpg"));
    assert_eq!(images[7].location, repo("media/5.webp"));
    assert!(images.iter().all(|image| !image.allow_fetch));
}

fn repo(path: &str) -> Location {
    Location::Repo(path.into())
}

const UUID: &str = "9e2ab0ff-bbf4-4f08-bed4-1e84f4f2c0f6";

#[test]
fn html_img_tags_become_images_under_the_same_rules_and_other_html_is_dropped() {
    let markdown = format!(
        "# Cam\n\n<img width=\"921\" alt=\"Zoom [view]\" src=\"https://github.com/user-attachments/assets/{UUID}\" />\n\
         <IMG SRC='media/shot.png'>\n\n\
         Inline <img src=https://raw.githubusercontent.com/{REPO}/{SHA}/a.png alt=x> text.\n\n\
         <p><img src=\"https://example.test/track.png\"><b>bold</b></p>\n\n\
         <img src=\"https://github.com/user-attachments/assets/{UUID}?x=1\">\n\n\
         <img src=\"a b.png\"><img src=\"x>y.png\"><imgx src=\"no.png\">\n"
    );
    let readme = Saved {
        path: "README.md".into(),
        markdown,
    }
    .parse(REPO, SHA, true);
    for tag in ["<img", "<IMG", "<p>", "<b>"] {
        assert!(!readme.markdown().contains(tag), "{}", readme.markdown());
    }
    assert!(!readme.markdown().contains("bold"));
    let images = readme.images();
    let found: Vec<_> = images.iter().map(|i| (i.alt(), &i.location)).collect();
    assert_eq!(
        found,
        [
            ("Zoom [view]", &Location::Attachment(UUID.into())),
            ("", &repo("media/shot.png")),
            ("x", &repo("a.png")),
        ]
    );
    assert_eq!(
        images[0].destination(),
        format!("https://github.com/user-attachments/assets/{UUID}")
    );
}

#[test]
fn only_real_img_elements_count_and_a_block_tag_may_span_lines() {
    let markdown = "<!-- <img src=\"media/commented.png\"> -->\n\n\
         <script>let s = '</scriptish><img src=\"media/script.png\">';</script>\n\n\
         <img-preview src=\"media/preview.png\"></img-preview>\n\n\
         <a title='<img src=\"media/attribute.png\">' href=\"x\">link</a>\n\n\
         <p align=\"center\">\n  <img\n    alt=\"A &amp; B &#x2014; &lt;C&gt;\"\n    src=\"media/split.png\"\n  />\n</p>\n"
        .to_owned();
    let readme = Saved {
        path: "README.md".into(),
        markdown,
    }
    .parse(REPO, SHA, true);
    let found: Vec<_> = readme
        .images()
        .iter()
        .map(|i| (i.alt(), &i.location))
        .collect();
    assert_eq!(found, [("A & B \u{2014} <C>", &repo("media/split.png"))]);
}

#[test]
fn only_github_attachment_ids_are_attachments() {
    let at = |dest: &str| image_location(REPO, SHA, "README.md", dest);
    assert_eq!(
        at(&format!(
            "https://github.com/user-attachments/assets/{UUID}"
        )),
        Some(Location::Attachment(UUID.into()))
    );
    for refused in [
        "https://github.com/user-attachments/assets/".to_owned(),
        format!("https://github.com/user-attachments/assets/{UUID}/x"),
        format!("https://github.com/user-attachments/assets/{UUID}?raw=1"),
        "https://github.com/user-attachments/assets/9e2ab0ff-bbf4-4f08-bed4".to_owned(),
        "https://github.com/user-attachments/assets/zzzzzzzz-bbf4-4f08-bed4-1e84f4f2c0f6"
            .to_owned(),
        format!("http://github.com/user-attachments/assets/{UUID}"),
        format!("https://github.com/user-attachments/files/{UUID}"),
        format!("https://evil.test/user-attachments/assets/{UUID}"),
        format!("https://raw.githubusercontent.com/someone/else/{SHA}/a.png"),
        format!("https://github.com/someone/else/blob/{SHA}/a.png"),
    ] {
        assert_eq!(at(&refused), None, "{refused}");
    }
    assert_eq!(
        at(&format!("https://github.com/{REPO}/blob/{SHA}/docs/a.png")),
        Some(repo("docs/a.png"))
    );
}

#[test]
fn an_attachment_follows_one_redirect_only_to_github_asset_hosts() {
    for allowed in [
        "https://github-production-user-asset-6210df.s3.amazonaws.com/1/2.png?X-Amz-Expires=300",
        "https://private-user-images.githubusercontent.com/1/2.png?jwt=x",
        "https://objects.githubusercontent.com/a",
    ] {
        assert!(asset_redirect(allowed), "{allowed}");
    }
    for refused in [
        "http://github-production-user-asset-6210df.s3.amazonaws.com/1.png",
        "https://github-production-user-asset-.s3.amazonaws.com/1.png",
        "https://evil.s3.amazonaws.com/1.png",
        "https://github-production-user-asset-6210df.s3.amazonaws.com.evil.test/1.png",
        "https://githubusercontent.com.evil.test/1.png",
        "https://evilgithubusercontent.com/1.png",
        "https://user@objects.githubusercontent.com/1.png",
        "https://objects.githubusercontent.com:8443/1.png",
        "https://github.com/user-attachments/assets/x",
        "/relative.png",
        "",
    ] {
        assert!(!asset_redirect(refused), "{refused}");
    }
}

#[test]
fn bytes_not_file_extensions_determine_if_a_bounded_still_image_is_safe() {
    let png = |width| {
        let mut bytes = Cursor::new(Vec::new());
        image::DynamicImage::new_rgb8(width, 1)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    };
    assert_eq!(check_image(&png(2048)).unwrap(), "png");
    assert!(check_image(&png(2049)).is_err());
    let mut too_big = png(16);
    too_big.resize(MAX_IMAGE as usize + 1, 0);
    assert!(check_image(&too_big).is_err());
    assert!(check_image(b"GIF89a").is_err());
    assert!(check_image(b"<svg></svg>").is_err());
    assert!(markdown_file("docs/README.MD"));
    assert!(!markdown_file("README.html"));
}
