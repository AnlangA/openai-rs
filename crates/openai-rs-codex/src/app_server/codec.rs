use tokio::io::{AsyncBufRead, AsyncBufReadExt};

use crate::{ConnectionFailure, ConnectionFailureKind};

/// Read one JSONL frame without ever growing the allocation beyond `limit`.
pub(crate) async fn read_bounded_line<R>(
    reader: &mut R,
    limit: usize,
) -> Result<Option<Vec<u8>>, ConnectionFailure>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::new();

    loop {
        let available = reader.fill_buf().await.map_err(|error| {
            ConnectionFailure::new(
                ConnectionFailureKind::Io,
                format!("could not read app-server stdout: {error}"),
            )
        })?;

        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            return Err(ConnectionFailure::new(
                ConnectionFailureKind::EndOfFile,
                "app-server stdout ended in the middle of a JSONL frame",
            ));
        }

        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        let payload_len = newline.unwrap_or(available.len());

        if line.len().saturating_add(payload_len) > limit {
            return Err(ConnectionFailure::new(
                ConnectionFailureKind::LineTooLong,
                format!("app-server JSONL frame exceeded the {limit}-byte limit"),
            ));
        }

        line.extend_from_slice(&available[..payload_len]);
        reader.consume(consumed);

        if newline.is_some() {
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            return Ok(Some(line));
        }
    }
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncWriteExt, BufReader, duplex};

    use super::read_bounded_line;
    use crate::ConnectionFailureKind;

    #[tokio::test]
    async fn reads_fragmented_line() -> Result<(), Box<dyn std::error::Error>> {
        let (mut writer, reader) = duplex(8);
        let task = tokio::spawn(async move {
            writer.write_all(b"{\"id\":").await?;
            writer.write_all(b"1}\r\n").await?;
            Ok::<_, std::io::Error>(())
        });
        let mut reader = BufReader::with_capacity(3, reader);
        let line = read_bounded_line(&mut reader, 64)
            .await?
            .ok_or("expected a line")?;
        assert_eq!(line, br#"{"id":1}"#);
        task.await??;
        Ok(())
    }

    #[tokio::test]
    async fn rejects_oversize_line_before_growth() -> Result<(), Box<dyn std::error::Error>> {
        let (mut writer, reader) = duplex(64);
        writer.write_all(b"123456789\n").await?;
        let mut reader = BufReader::with_capacity(4, reader);
        let error = read_bounded_line(&mut reader, 8)
            .await
            .err()
            .ok_or("expected an oversized-line error")?;
        assert_eq!(error.kind, ConnectionFailureKind::LineTooLong);
        Ok(())
    }
}
