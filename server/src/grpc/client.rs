//! gRPC client helpers implementation

// pub use svc_storage_client_grpc::adsb::rpc_service_client::RpcServiceClient as AdsbClient;
use futures::lock::Mutex;
use std::sync::Arc;
pub use tonic::transport::Channel;

/// Struct to hold all gRPC client connections
#[derive(Clone, Debug)]
#[allow(missing_copy_implementations)]
pub struct GrpcClients {
    // pub adsb: GrpcClient<AdsbClient<Channel>>,
}

/// Struct to define a gRPC client
/// Allow dead code in R3, doesn't have access to other services yet
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct GrpcClient<T> {
    inner: Arc<Mutex<Option<T>>>,
    address: String,
}

/// Returns a string in http://host:port format from provided
/// environment variables
fn get_grpc_endpoint(env_host: &str, env_port: &str) -> String {
    grpc_debug!("(get_grpc_endpoint) entry.");
    let port = match std::env::var(env_port) {
        Ok(s) => s,
        Err(_) => {
            grpc_error!("(env) {} undefined.", env_port);
            "".to_string()
        }
    };
    let host = match std::env::var(env_host) {
        Ok(s) => s,
        Err(_) => {
            grpc_error!("(env) {} undefined.", env_host);
            "".to_string()
        }
    };

    let full = format!("http://{host}:{port}");
    grpc_info!("(get_grpc_endpoint) full address: {}", full);
    full
}

impl<T> GrpcClient<T> {
    /// Invalidates a gRPC client by setting it to [`None`]
    pub async fn invalidate(&mut self) {
        let arc = Arc::clone(&self.inner);
        let mut client = arc.lock().await;
        *client = None;
    }

    /// Creates a new gRPC client object
    pub fn new(env_host: &str, env_port: &str) -> Self {
        let opt: Option<T> = None;
        GrpcClient {
            inner: Arc::new(Mutex::new(opt)),
            address: get_grpc_endpoint(env_host, env_port),
        }
    }
}

#[allow(unused_macros)]
macro_rules! grpc_client {
    ( $client: ident, $name: expr ) => {
        impl GrpcClient<$client<Channel>> {
            pub async fn get_client(&mut self) -> Option<$client<Channel>> {
                grpc_debug!("(get_client) {} entry.", $name);

                let arc = Arc::clone(&self.inner);

                // if already connected, return the client
                let client = arc.lock().await;
                if client.is_some() {
                    return client.clone();
                }

                grpc_debug!(
                    "(grpc) connecting to {} server at {}",
                    $name,
                    self.address.clone()
                );
                let result = $client::connect(self.address.clone()).await;
                match result {
                    Ok(client) => {
                        grpc_info!(
                            "(grpc) success: connected to {} server at {}",
                            $name,
                            self.address.clone()
                        );
                        Some(client)
                    }
                    Err(e) => {
                        grpc_error!(
                            "(grpc) couldn't connect to {} server at {}; {}",
                            $name,
                            self.address,
                            e
                        );
                        None
                    }
                }
            }
        }
    };
}

// grpc_client!(AdsbClient, "adsb");

impl Default for GrpcClients {
    /// Creates default clients
    fn default() -> Self {
        GrpcClients {
            // adsb: GrpcClient::<AdsbClient<Channel>>::new("ADSB_HOST_GRPC", "ADSB_PORT_GRPC"),
        }
    }
}
