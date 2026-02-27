use axum::{
    extract::ConnectInfo,
    http::{Request, StatusCode},
    response::{IntoResponse, Response},
};
use std::collections::HashMap;
use std::future::Future;
use std::net::{IpAddr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Instant;
use tokio::sync::Mutex;
use tower::{Layer, Service};

#[derive(Clone)]
pub struct RateLimitLayer {
    state: Arc<Mutex<SlidingWindowState>>,
    max_requests: u32,
    window_secs: u64,
}

struct SlidingWindowState {
    windows: HashMap<IpAddr, Vec<Instant>>,
}

impl RateLimitLayer {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        Self {
            state: Arc::new(Mutex::new(SlidingWindowState {
                windows: HashMap::new(),
            })),
            max_requests,
            window_secs,
        }
    }
}

impl<S> Layer<S> for RateLimitLayer {
    type Service = RateLimitService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RateLimitService {
            inner,
            state: self.state.clone(),
            max_requests: self.max_requests,
            window_secs: self.window_secs,
        }
    }
}

#[derive(Clone)]
pub struct RateLimitService<S> {
    inner: S,
    state: Arc<Mutex<SlidingWindowState>>,
    max_requests: u32,
    window_secs: u64,
}

impl<S, B> Service<Request<B>> for RateLimitService<S>
where
    S: Service<Request<B>, Response = Response> + Clone + Send + 'static,
    S::Future: Send,
    B: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let ip = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip());

        let state = self.state.clone();
        let max = self.max_requests;
        let window = self.window_secs;
        let mut inner = self.inner.clone();

        Box::pin(async move {
            if let Some(ip) = ip {
                if ip.is_loopback() {
                    return inner.call(req).await;
                }
                let mut s = state.lock().await;
                let now = Instant::now();
                let cutoff = now - std::time::Duration::from_secs(window);
                let entries = s.windows.entry(ip).or_default();
                entries.retain(|t| *t > cutoff);
                if entries.len() >= max as usize {
                    return Ok(
                        (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response()
                    );
                }
                entries.push(now);
                // Periodically prune IPs with no recent requests
                if s.windows.len() > 10_000 {
                    s.windows.retain(|_, v| !v.is_empty());
                }
            }
            inner.call(req).await
        })
    }
}
