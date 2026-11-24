//! 生成した service（smithy-rs・http-1x）を、axum の Router の fallback に入れるための包み。
//!
//! - axum の `fallback_service` は `Sync` を求めるが、生成した service は `Send` だけなので、`Mutex` に入れて要求ごとに写しを取る。
//!   鍵は写しを取る間だけ握り、await の前に放す（要求どうしは待ち合わない）。
//! - 生成した service は本文の誤りを `axum::Error` から変えられないので、本文を `Box<dyn Error>` の誤りの形に替える。
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use axum::extract::Request;
use axum::response::Response;
use http_body_util::BodyExt;
use tower::ServiceExt;
use tower::util::BoxCloneService;

type BoxError = Box<dyn std::error::Error + Send + Sync + 'static>;
type GeneratedBody = http_body_util::combinators::UnsyncBoxBody<bytes::Bytes, BoxError>;

/// 生成した service の包み。`Router::fallback_service` に渡す。
#[derive(Clone)]
pub struct Generated(Arc<Mutex<BoxCloneService<Request, Response, Infallible>>>);

impl Generated {
    pub fn new<S, B>(service: S) -> Self
    where
        S: tower::Service<
                axum::http::Request<GeneratedBody>,
                Response = axum::http::Response<B>,
                Error = Infallible,
            > + Clone
            + Send
            + 'static,
        S::Future: Send + 'static,
        B: http_body::Body<Data = bytes::Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        let service = tower::ServiceBuilder::new()
            .map_request(|request: Request| {
                request.map(|body| body.map_err(|e| Box::new(e) as BoxError).boxed_unsync())
            })
            .map_response(|response: axum::http::Response<B>| response.map(axum::body::Body::new))
            .service(service);
        Self(Arc::new(Mutex::new(BoxCloneService::new(service))))
    }
}

impl tower::Service<Request> for Generated {
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        // 写しの側の ready は oneshot が待つ。
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let inner = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        Box::pin(inner.oneshot(request))
    }
}
