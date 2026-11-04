use pulldown_cmark::{Event, LinkType, Parser, Tag};

/// Canonical inline Markdown links are role references. Plain @names, code,
/// images, HTML and reference-style links do not carry collaboration identity.
pub(super) fn role_mentions(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    for event in Parser::new(text) {
        if let Event::Start(Tag::Link {
            link_type: LinkType::Inline,
            dest_url,
            ..
        }) = event
            && let Some(id) = dest_url.strip_prefix("mention://role/")
            && !id.is_empty()
            && id.len() <= 128
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
            && !ids.iter().any(|item| item == id)
        {
            ids.push(id.to_owned());
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_canonical_links_identify_roles() {
        assert_eq!(
            role_mentions(
                "[@开发](mention://role/dev) then [@审查](mention://role/review) [@again](mention://role/dev)"
            ),
            ["dev", "review"]
        );
        assert!(role_mentions("@dev `[@dev](mention://role/dev)`\n\n```md\n[@review](mention://role/review)\n```\n![avatar](mention://role/dev)\n[x][r]\n\n[r]: mention://role/dev").is_empty());
        assert!(
            role_mentions("[x](mention://role/../../bad) [x](mention://role/dev?other=1)")
                .is_empty()
        );
    }
}
