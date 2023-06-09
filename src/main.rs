use hyper::{Body, Request, Response, Server, service::{make_service_fn, service_fn}};
use serde_json::{Value, json};
use jsonrpc::{Client, error::RpcError};
use jsonrpc::simple_http::{self, SimpleHttpTransport};
use serde_json::value::RawValue;
use std::sync::Arc;

mod allowlist;

struct VerusRPC {
    client: Arc<Client>,
}

impl VerusRPC {
    fn new(url: &str, user: &str, pass: &str) -> Result<VerusRPC, simple_http::Error> {
        let transport = SimpleHttpTransport::builder()
            .url(url)?
            .auth(user, Some(pass))
            .build();
        Ok(VerusRPC { client: Arc::new(Client::with_transport(transport)) })
    }

    fn handle(&self, req_body: Value) -> Result<Value, RpcError> {
        let method = req_body["method"].as_str().unwrap();
        let params: Vec<Box<RawValue>> = req_body["params"].as_array().unwrap().iter().map(|v| RawValue::from_string(v.to_string()).unwrap()).collect();
    
        if !allowlist::is_method_allowed(method, &params) {
            return Err(RpcError { code: -32601, message: "Method not found".into(), data: None });
        }
    
        let request = self.client.build_request(method, &params);
        let response = self.client.send_request(request).map_err(|e| match e {
            jsonrpc::Error::Rpc(rpc_error) => rpc_error,
            _ => RpcError { code: -32603, message: "Internal error".into(), data: None },
        })?;
        let result: Value = response.result().map_err(|e| match e {
            jsonrpc::Error::Rpc(rpc_error) => rpc_error,
            _ => RpcError { code: -32603, message: "Internal error".into(), data: None },
        })?;
        Ok(result)
    }
}

async fn handle_req(req: Request<Body>, rpc: Arc<VerusRPC>) -> Result<Response<Body>, hyper::Error> {
    let whole_body = hyper::body::to_bytes(req.into_body()).await?;
    let str_body = String::from_utf8(whole_body.to_vec()).unwrap();
    let json_body: Value = serde_json::from_str(&str_body).unwrap();
    let result = rpc.handle(json_body);
    match result {
        Ok(res) => Ok(Response::new(Body::from(json!({"result": res}).to_string()))),
        Err(err) => Ok(Response::new(Body::from(json!({"error": { "code": err.code, "message": err.message }}).to_string()))),
    }
}

#[tokio::main]
async fn main() {
    let mut settings = config::Config::default();
    
    settings.merge(config::File::with_name("Conf")).expect("Failed to open configuration file");

    let url = settings.get_str("rpc_url").expect("Failed to read 'rpc_url' from configuration");
    let user = settings.get_str("rpc_user").expect("Failed to read 'rpc_user' from configuration");
    let password = settings.get_str("rpc_password").expect("Failed to read 'rpc_password' from configuration");
    
    let port = settings.get_str("server_port").expect("Failed to read 'server_port' from configuration");

    let addr = ([127, 0, 0, 1], port.parse().unwrap()).into();

    let make_svc = make_service_fn(|_conn| {
        let rpc = Arc::new(VerusRPC::new(&url, &user, &password).unwrap());
        async {
            Ok::<_, hyper::Error>(service_fn(move |req| handle_req(req, rpc.clone())))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    if let Err(e) = server.await {
        eprintln!("server error: {}", e);
    }
}