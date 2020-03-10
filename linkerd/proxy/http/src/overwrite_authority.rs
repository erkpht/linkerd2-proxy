use super::h1;
use futures::{try_ready, Future, Poll};
use http;
use http::header::HOST;
use http::uri::Authority;
use tracing::trace;

pub trait ExtractAuthority<T> {
    fn extract(&self, target: &T) -> Option<Authority>;
}

pub trait ForceAbsForm {
    fn abs_form(&self) -> bool;
}

#[derive(Debug, Clone)]
pub struct Layer<E> {
    extractor: E,
}

#[derive(Clone, Debug)]
pub struct MakeSvc<E, M> {
    extractor: E,
    inner: M,
}

pub struct MakeSvcFut<M> {
    authority: Option<Authority>,
    inner: M,
}

#[derive(Clone, Debug)]
pub struct Service<S> {
    authority: Option<Authority>,
    inner: S,
}

// === impl Layer ===

pub fn layer<E>(extractor: E) -> Layer<E>
where
    E: Clone,
{
    Layer { extractor }
}

impl<E, M> tower::layer::Layer<M> for Layer<E>
where
    E: Clone,
{
    type Service = MakeSvc<E, M>;

    fn layer(&self, inner: M) -> Self::Service {
        MakeSvc {
            extractor: self.extractor.clone(),
            inner,
        }
    }
}

impl<E, T, M> tower::Service<T> for MakeSvc<E, M>
where
    T: Clone + Send + Sync + 'static,
    M: tower::Service<T>,
    E: ExtractAuthority<T>,
    E: Clone,
{
    type Response = Service<M::Response>;
    type Error = M::Error;
    type Future = MakeSvcFut<M::Future>;

    fn poll_ready(&mut self) -> Poll<(), M::Error> {
        self.inner.poll_ready()
    }

    fn call(&mut self, t: T) -> Self::Future {
        let authority = self.extractor.extract(&t);
        let inner = self.inner.call(t);
        MakeSvcFut { authority, inner }
    }
}

impl<F> Future for MakeSvcFut<F>
where
    F: Future,
{
    type Item = Service<F::Item>;
    type Error = F::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let inner = try_ready!(self.inner.poll());
        Ok(Service {
            authority: self.authority.clone(),
            inner,
        }
        .into())
    }
}

// === impl Service ===

impl<S, B> tower::Service<http::Request<B>> for Service<S>
where
    S: tower::Service<http::Request<B>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.inner.poll_ready()
    }

    fn call(&mut self, mut req: http::Request<B>) -> Self::Future {
        if let Some(new_authority) = self.authority.clone() {
            trace!(%new_authority, "Overwriting authority");
            h1::set_authority(req.uri_mut(), new_authority.clone());
            // remove host header and let hyper set it based on URI authority
            if let Some(original_host) = req.headers_mut().remove(HOST) {
                trace!("Removed original host header {:?}", original_host);
            }
        }
        self.inner.call(req)
    }
}
