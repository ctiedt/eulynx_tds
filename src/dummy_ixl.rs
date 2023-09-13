use std::sync::Arc;

use futures::stream::BoxStream;
use miette::IntoDiagnostic;
use rasta::{rasta_server::RastaServer, SciPacket};
use sci_rs::SCITelegram;
use tokio::sync::{broadcast::Sender, Mutex};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tonic::{transport::Server, Request, Response, Status, Streaming};

mod rasta {
    tonic::include_proto!("sci");
}

struct TdsServer {
    outgoing_messages: Sender<SciPacket>,
    incoming_messages: Arc<Mutex<Option<BoxStream<'static, SciPacket>>>>,
}

impl TdsServer {
    pub fn new() -> Self {
        let (tx, _) = tokio::sync::broadcast::channel(10);
        Self {
            outgoing_messages: tx,
            incoming_messages: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn send(&self, message: SCITelegram) {}
}

#[tonic::async_trait]
impl rasta::rasta_server::Rasta for TdsServer {
    type StreamStream = BoxStream<'static, Result<SciPacket, Status>>;
    //type StreamStream = BroadcastStream<Result<SciPacket, Status>>;

    async fn stream(
        &self,
        request: Request<Streaming<SciPacket>>,
    ) -> Result<Response<Self::StreamStream>, Status> {
        let req = request.into_inner().map(|m| m.unwrap());
        self.incoming_messages.lock().await.replace(Box::pin(req));

        Ok(Response::new(Box::pin(
            BroadcastStream::new(self.outgoing_messages.subscribe()).map(|m| Ok(m.unwrap())),
        )))
    }
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    let tds_server = TdsServer::new();
    let addr = "127.0.0.1:8001".parse().into_diagnostic()?;

    Server::builder()
        .add_service(RastaServer::new(tds_server))
        .serve(addr)
        .await
        .into_diagnostic()?;

    Ok(())
}
