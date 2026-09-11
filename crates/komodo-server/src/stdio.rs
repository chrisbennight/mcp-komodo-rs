//! Bounded newline-delimited MCP transport for a local status-only process.

use std::{io, sync::Arc, time::Duration};

use rmcp::{
    RoleServer,
    model::{ClientJsonRpcMessage, JsonRpcMessage, RequestId, ServerJsonRpcMessage},
    transport::Transport,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    sync::Mutex,
};

const MAX_FRAME_BYTES: usize = 64 * 1024;
const MAX_PENDING_REQUESTS: usize = 32;
const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// Reject malformed/oversized input without logging its content.
///
/// A partial input buffer survives cancellation of `receive`. Requests remain
/// counted until their response has been written, so a blocked stdout cannot
/// accumulate an unbounded response queue. This transport is local-only.
pub struct BoundedStdio<R, W> {
    read: BufReader<R>,
    line: Vec<u8>,
    write: Arc<Mutex<W>>,
    pending: Arc<std::sync::Mutex<Vec<RequestId>>>,
    closed: bool,
}

impl<R: AsyncRead, W> BoundedStdio<R, W> {
    #[must_use]
    pub fn new(read: R, write: W) -> Self {
        Self {
            read: BufReader::new(read),
            line: Vec::new(),
            write: Arc::new(Mutex::new(write)),
            pending: Arc::default(),
            closed: false,
        }
    }
}

impl<R, W> Transport<RoleServer> for BoundedStdio<R, W>
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    type Error = io::Error;

    fn send(
        &mut self,
        item: ServerJsonRpcMessage,
    ) -> impl Future<Output = Result<(), io::Error>> + Send + 'static {
        let write = Arc::clone(&self.write);
        let pending = Arc::clone(&self.pending);
        async move {
            let id = match &item {
                JsonRpcMessage::Response(response) => Some(response.id.clone()),
                JsonRpcMessage::Error(error) => error.id.clone(),
                _ => None,
            };
            let mut bytes = serde_json::to_vec(&item)
                .map_err(|_| io::Error::other("MCP response encoding failed"))?;
            bytes.push(b'\n');
            tokio::time::timeout(WRITE_TIMEOUT, async {
                let mut writer = write.lock().await;
                writer.write_all(&bytes).await?;
                writer.flush().await
            })
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "MCP output timed out"))??;
            if let Some(id) = id {
                pending
                    .lock()
                    .expect("pending request lock")
                    .retain(|entry| entry != &id);
            }
            Ok(())
        }
    }

    async fn receive(&mut self) -> Option<ClientJsonRpcMessage> {
        if self.closed {
            return None;
        }
        let remaining = MAX_FRAME_BYTES + 1 - self.line.len();
        let read = (&mut self.read)
            .take(remaining as u64)
            .read_until(b'\n', &mut self.line)
            .await;
        if !matches!(read, Ok(count) if count > 0)
            || self.line.len() > MAX_FRAME_BYTES
            || !self.line.ends_with(b"\n")
        {
            self.closed = true;
            return None;
        }
        let message = serde_json::from_slice::<ClientJsonRpcMessage>(&self.line).ok();
        self.line.clear();
        if let Some(JsonRpcMessage::Request(request)) = &message {
            let mut pending = self.pending.lock().expect("pending request lock");
            if pending.len() >= MAX_PENDING_REQUESTS || pending.contains(&request.id) {
                self.closed = true;
                return None;
            }
            pending.push(request.id.clone());
        }
        if message.is_none() {
            self.closed = true;
        }
        message
    }

    async fn close(&mut self) -> Result<(), io::Error> {
        self.closed = true;
        tokio::time::timeout(WRITE_TIMEOUT, async {
            self.write.lock().await.shutdown().await
        })
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "MCP close timed out"))?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::model::{EmptyResult, ServerResult};

    #[tokio::test]
    async fn malformed_oversized_and_unterminated_frames_close_without_echo() {
        for input in [
            b"not-json-sentinel\n".to_vec(),
            vec![b'x'; MAX_FRAME_BYTES + 1],
            b"{\"jsonrpc\":\"2.0\",\"method\":\"ping\",\"id\":1}".to_vec(),
        ] {
            let mut transport = BoundedStdio::new(std::io::Cursor::new(input), tokio::io::sink());
            assert!(transport.receive().await.is_none());
            assert!(transport.receive().await.is_none());
        }
    }

    #[tokio::test]
    async fn partial_frame_survives_receive_cancellation() {
        let (read, mut peer) = tokio::io::duplex(1024);
        let mut transport = BoundedStdio::new(read, tokio::io::sink());
        peer.write_all(b"{\"jsonrpc\":\"2.0\",").await.unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(10), transport.receive())
                .await
                .is_err()
        );
        peer.write_all(b"\"method\":\"ping\",\"id\":1}\n")
            .await
            .unwrap();
        assert!(matches!(
            transport.receive().await,
            Some(JsonRpcMessage::Request(_))
        ));
    }

    #[tokio::test]
    async fn pending_requests_are_bounded_and_responses_release_capacity() {
        use std::fmt::Write;
        let mut frames = String::new();
        for id in 0..=MAX_PENDING_REQUESTS {
            writeln!(
                frames,
                "{{\"jsonrpc\":\"2.0\",\"method\":\"ping\",\"id\":{id}}}"
            )
            .unwrap();
        }
        for release in [false, true] {
            let mut transport =
                BoundedStdio::new(std::io::Cursor::new(frames.clone()), tokio::io::sink());
            for _ in 0..MAX_PENDING_REQUESTS {
                assert!(transport.receive().await.is_some());
            }
            if release {
                transport
                    .send(ServerJsonRpcMessage::response(
                        ServerResult::EmptyResult(EmptyResult {}),
                        RequestId::Number(0),
                    ))
                    .await
                    .unwrap();
            }
            assert_eq!(transport.receive().await.is_some(), release);
        }
    }

    #[tokio::test]
    async fn duplicate_pending_request_ids_close_the_stream() {
        let frame = "{\"jsonrpc\":\"2.0\",\"method\":\"ping\",\"id\":1}\n";
        let mut transport =
            BoundedStdio::new(std::io::Cursor::new(frame.repeat(2)), tokio::io::sink());
        assert!(transport.receive().await.is_some());
        assert!(transport.receive().await.is_none());
    }
}
