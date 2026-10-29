//! Length-delimited JSON-RPC frame transport.
use std::io;

use serde::Serialize;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use super::{MAX_FRAME_BYTES, ProtocolError};

/// Read one bounded JSON-RPC line. A final unterminated line is accepted at EOF.
pub async fn read_frame<R>(reader: &mut R) -> io::Result<Option<Vec<u8>>>
where
    R: AsyncBufRead + Unpin,
{
    let mut frame = Vec::with_capacity(1024);
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return if frame.is_empty() { Ok(None) } else { Ok(Some(frame)) };
        }

        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        let content_end = if newline.is_some() && available.get(take.saturating_sub(1)) == Some(&b'\n') {
            take.saturating_sub(1)
        } else {
            take
        };
        let content_end = if content_end > 0 && available.get(content_end.saturating_sub(1)) == Some(&b'\r') {
            content_end.saturating_sub(1)
        } else {
            content_end
        };

        if frame.len().saturating_add(content_end) > MAX_FRAME_BYTES {
            reader.consume(take);
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                ProtocolError::FrameTooLarge { limit: MAX_FRAME_BYTES },
            ));
        }
        let content = available
            .get(..content_end)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid buffered frame boundary"))?;
        frame.extend_from_slice(content);
        reader.consume(take);

        if newline.is_some() {
            return Ok(Some(frame));
        }
    }
}

/// Write one compact JSON-RPC line and flush it to the client.
pub async fn write_frame<W, T>(writer: &mut W, message: &T) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let encoded = serde_json::to_vec(message).map_err(io::Error::other)?;
    if encoded.len() > MAX_FRAME_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            ProtocolError::FrameTooLarge { limit: MAX_FRAME_BYTES },
        ));
    }
    writer.write_all(&encoded).await?;
    writer.write_all(b"\n").await?;
    writer.flush().await
}
