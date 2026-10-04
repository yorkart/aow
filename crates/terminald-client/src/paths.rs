use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};

const PATH_SEGMENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~');

pub(super) fn runtime_path(id: &str) -> String {
    format!("/v1/runtimes/{}", encode_id(id))
}

pub(super) fn attach_uri(
    id: &str,
    epoch: Option<&str>,
    after: Option<u64>,
    controlled: bool,
    vt_snapshot: bool,
    observer: bool,
) -> String {
    let uri = format!("ws://aow-terminald/v1/runtimes/{}/attach", encode_id(id));
    let mut uri = match (epoch, after) {
        (Some(epoch), Some(after)) => {
            format!("{uri}?epoch={}&after={after}", encode_id(epoch))
        }
        _ => uri,
    };
    if controlled {
        let separator = if uri.contains('?') { '&' } else { '?' };
        uri = format!("{uri}{separator}control=v2");
    }
    if vt_snapshot {
        let separator = if uri.contains('?') { '&' } else { '?' };
        uri = format!("{uri}{separator}capabilities=vt-snapshot-v1");
    }
    if observer {
        let separator = if uri.contains('?') { '&' } else { '?' };
        uri = format!("{uri}{separator}observer=v1");
    }
    uri
}

pub(super) fn encode_id(id: &str) -> String {
    utf8_percent_encode(id, PATH_SEGMENT).to_string()
}
