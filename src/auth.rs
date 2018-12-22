use crate::consts::*;
use crate::data::{AppCred, AuthPayload, AuthResp};
use futures::future;
use futures::future::{Future, FutureResult};
use futures::sync::oneshot;
use hyper::server::{conn, Server};
use hyper::service::{MakeService, Service};
use hyper::{Body, Request, Response};
use reqwest;
use reqwest::r#async::Client;
use std::borrow::Cow;
use std::cell::RefCell;
use std::ops::Deref;
use std::str;
use url::form_urlencoded;

struct CodeService {
    sender: RefCell<Option<oneshot::Sender<String>>>,
}

impl Service for CodeService {
    type ReqBody = Body;
    type ResBody = Body;
    type Error = !;
    type Future = FutureResult<Response<Body>, !>;

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        if let Some(inner) = self.sender.replace(None) {
            let queries = form_urlencoded::parse(req.uri().query().unwrap_or("").as_bytes());

            for (k, v) in queries {
                if k == Cow::Borrowed("code") {
                    inner.send(v.to_string()).unwrap();
                    break;
                }
            }
        };

        future::ok(Response::new(Body::from(
            "<body onload=\"window.close()\"></body>",
        )))
    }
}

struct MkCodeService {
    sender: RefCell<Option<oneshot::Sender<String>>>,
}

impl MkCodeService {
    fn new(sender: oneshot::Sender<String>) -> MkCodeService {
        MkCodeService {
            sender: RefCell::new(Some(sender)),
        }
    }
}

impl MakeService<&conn::AddrStream> for MkCodeService {
    type ResBody = Body;
    type ReqBody = Body;
    type Error = !;
    type Service = CodeService;
    type MakeError = !;
    type Future = FutureResult<Self::Service, !>;

    fn make_service(&mut self, _: &conn::AddrStream) -> Self::Future {
        future::ok(CodeService {
            sender: RefCell::new(self.sender.replace(None)),
        })
    }
}

#[derive(Debug)]
pub enum RequestCodeError {
    Server(hyper::error::Error),
    Channel,
}

pub fn request_code(
    client_id: &str,
) -> impl Future<Item = (String, String), Error = RequestCodeError> {
    let port = 8478;

    let (p, c) = oneshot::channel::<String>();

    let recv = c.shared();
    let shutdown = recv.clone().map(|_| ());

    let addr = &([127, 0, 0, 1], port).into();

    let factory = MkCodeService::new(p);

    let server = Server::bind(addr)
        .serve(factory)
        .with_graceful_shutdown(shutdown)
        .map_err(|e| RequestCodeError::Server(e));

    let redirect = format!("http://localhost:{}/", port);

    let uri = format!(
        "{}?client_id={}&response_type=code&redirect_uri={}",
        OAUTH_AUTHORIZE,
        client_id,
        redirect.clone()
    );

    println!("Goto {}", uri);

    return recv
        .map_err(|_| RequestCodeError::Channel)
        .join(server)
        .map(|(result, _)| (result.deref().clone(), redirect));
}

fn fetch_code(payload: AuthPayload) -> impl Future<Item = AuthResp, Error = reqwest::Error> {
    let client = Client::new();
    let pending = client.post(OAUTH_ACCESS_TOKEN).json(&payload).send();

    pending.and_then(|mut resp| resp.json())
}

pub fn request_token(
    app_cred: AppCred,
    code: String,
    redirect: String,
) -> impl Future<Item = AuthResp, Error = reqwest::Error> {
    fetch_code(AuthPayload::AuthorizationCode {
        app_cred: app_cred,
        code: code,
        redirect_uri: redirect,
        state: None,
    })
}

pub fn refresh_token(
    app_cred: AppCred,
    refresh: String,
    redirect: String,
) -> impl Future<Item = AuthResp, Error = reqwest::Error> {
    fetch_code(AuthPayload::RefreshToken {
        app_cred: app_cred,
        refresh_token: refresh,
        redirect_uri: redirect,
    })
}
