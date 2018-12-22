use bgmtv::auth::{request_code, request_token};
use bgmtv::data::AppCred;
use futures::future::Future;
use tokio;

const CLIENT_ID: &str = env!("CLIENT_ID");
const CLIENT_SECRET: &str = env!("CLIENT_SECRET");

fn main() {
    let fut = request_code(CLIENT_ID)
        .map_err(|e| println!("{:#?}", e))
        .and_then(|(code, redirect)| {
            println!("Code: {}, redirect: {}", code, redirect);
            request_token(
                AppCred::new(CLIENT_ID.to_owned(), CLIENT_SECRET.to_owned()),
                code,
                redirect,
            )
            .map_err(|e| println!("{}", e))
        })
        .map(|resp| println!("{:#?}", resp));
    tokio::run(fut);
}
