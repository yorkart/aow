use serde_json::Value;
use std::io;
#[cfg(test)]
use tokio::io::AsyncWriteExt;
use tokio::io::{AsyncRead, AsyncReadExt};

use super::super::config::MAX_CACHED_SNAPSHOT_BYTES;

// Keep aligned with vt-worker/src/framing.mjs.
pub(super) const PROTOCOL_VERSION: u64 = 1;
pub(super) const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024 + 64 * 1024;
pub(super) const MAX_REQUEST_METADATA_BYTES: usize = 64 * 1024;

pub(in crate::vt_worker) struct Response {
    pub(in crate::vt_worker) metadata: Value,
    pub(in crate::vt_worker) data: Vec<u8>,
}

pub(super) async fn read_frame<R: AsyncRead + Unpin>(reader: &mut R) -> io::Result<Response> {
    let invalid = |message| io::Error::new(io::ErrorKind::InvalidData, message);
    let mut prefix = [0; 8];
    reader.read_exact(&mut prefix).await?;
    let frame_length = u32::from_be_bytes(prefix[..4].try_into().unwrap()) as usize;
    let metadata_length = u32::from_be_bytes(prefix[4..].try_into().unwrap()) as usize;
    if !(4..=MAX_FRAME_BYTES).contains(&frame_length)
        || metadata_length == 0
        || metadata_length > frame_length - 4
    {
        return Err(invalid("invalid VT frame lengths"));
    }
    let data_length = frame_length - 4 - metadata_length;
    if data_length > MAX_CACHED_SNAPSHOT_BYTES {
        return Err(invalid("VT response payload exceeds snapshot limit"));
    }
    let mut metadata = vec![0; metadata_length];
    reader.read_exact(&mut metadata).await?;
    let metadata: Value =
        serde_json::from_slice(&metadata).map_err(|_| invalid("invalid VT response JSON"))?;
    if metadata.get("protocol_version").and_then(Value::as_u64) != Some(PROTOCOL_VERSION) {
        return Err(invalid("unsupported VT protocol version"));
    }
    let mut data = vec![0; data_length];
    reader.read_exact(&mut data).await?;
    Ok(Response { metadata, data })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn frame(metadata: Value, data: &[u8]) -> Vec<u8> {
        let metadata = serde_json::to_vec(&metadata).unwrap();
        let mut result = Vec::new();
        result.extend_from_slice(&((4 + metadata.len() + data.len()) as u32).to_be_bytes());
        result.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
        result.extend_from_slice(&metadata);
        result.extend_from_slice(data);
        result
    }

    #[tokio::test]
    async fn frames_preserve_binary_payloads_across_partial_reads_and_concatenation() {
        let metadata = json!({ "protocol_version": PROTOCOL_VERSION, "id": 1 });
        let data = b"\0\n\r\x1b\xff\xc3\xa9";
        let mut wire = frame(metadata.clone(), data);
        wire.extend(frame(metadata.clone(), &[]));
        let (mut writer, mut reader) = tokio::io::duplex(7);
        let task = tokio::spawn(async move {
            for byte in wire {
                writer.write_all(&[byte]).await.unwrap();
            }
        });
        let first = read_frame(&mut reader).await.unwrap();
        assert_eq!(first.metadata, metadata);
        assert_eq!(first.data, data);
        assert!(read_frame(&mut reader).await.unwrap().data.is_empty());
        task.await.unwrap();
        assert_eq!(
            read_frame(&mut reader).await.err().unwrap().kind(),
            io::ErrorKind::UnexpectedEof
        );
    }

    #[tokio::test]
    async fn invalid_lengths_are_rejected_before_reading_or_allocating_the_body() {
        for (length, metadata) in [
            (0, 0),
            (3, 1),
            (5, 2),
            (5, 0),
            (u32::MAX, 1),
            ((MAX_FRAME_BYTES + 1) as u32, 1),
            ((MAX_CACHED_SNAPSHOT_BYTES + 6) as u32, 1),
        ] {
            let mut prefix = Vec::new();
            prefix.extend_from_slice(&length.to_be_bytes());
            prefix.extend_from_slice(&(metadata as u32).to_be_bytes());
            assert_eq!(
                read_frame(&mut prefix.as_slice())
                    .await
                    .err()
                    .unwrap()
                    .kind(),
                io::ErrorKind::InvalidData
            );
        }
    }

    #[tokio::test]
    async fn truncated_frames_and_wrong_protocol_versions_are_rejected() {
        let wire = frame(json!({ "protocol_version": PROTOCOL_VERSION }), b"payload");
        for end in 0..wire.len() {
            assert_eq!(
                read_frame(&mut &wire[..end]).await.err().unwrap().kind(),
                io::ErrorKind::UnexpectedEof
            );
        }
        let wire = frame(json!({ "protocol_version": 999 }), &[]);
        assert_eq!(
            read_frame(&mut wire.as_slice()).await.err().unwrap().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
