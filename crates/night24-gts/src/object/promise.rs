//! Promise values for the async model (single-threaded).
//!
//! A Promise resolves/rejects exactly once. Resolution is driven by the
//! VirtualMachine's async-completion queue (`drain_async_completions`), which
//! calls `resolve`/`reject` on the VM thread. Pending continuations
//! (`.then`/`async`/`await`) are invoked synchronously at settle time.

use std::cell::RefCell;
use std::rc::Rc;

use super::value::Object;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

struct PromiseInner {
    state: PromiseState,
    value: Option<Object>,
    continuations: Vec<PromiseContinuation>,
}

pub type PromiseContinuation = Box<dyn FnOnce(PromiseState, Object) + 'static>;

/// A Promise value.
pub struct Promise {
    inner: RefCell<PromiseInner>,
}

impl Promise {
    pub fn new() -> Rc<Promise> {
        Rc::new(Promise {
            inner: RefCell::new(PromiseInner {
                state: PromiseState::Pending,
                value: None,
                continuations: Vec::new(),
            }),
        })
    }

    pub fn state(&self) -> PromiseState {
        self.inner.borrow().state
    }

    pub fn resolve(&self, value: Object) {
        self.settle(PromiseState::Fulfilled, value);
    }

    pub fn reject(&self, reason: Object) {
        self.settle(PromiseState::Rejected, reason);
    }

    fn settle(&self, state: PromiseState, value: Object) {
        let mut g = self.inner.borrow_mut();
        if g.state != PromiseState::Pending {
            return;
        }
        g.state = state;
        g.value = Some(value.clone());
        let continuations = std::mem::take(&mut g.continuations);
        drop(g);
        for continuation in continuations {
            continuation(state, value.clone());
        }
    }

    pub fn add_continuation(&self, continuation: PromiseContinuation) {
        let mut g = self.inner.borrow_mut();
        if g.state == PromiseState::Pending {
            g.continuations.push(continuation);
            return;
        }
        let state = g.state;
        let value = g.value.clone().unwrap_or(Object::Undefined);
        drop(g);
        continuation(state, value);
    }

    /// Block until settled, returning the resolution value or rejection reason.
    pub fn wait(&self) -> Object {
        while self.state() == PromiseState::Pending {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        self.inner
            .borrow()
            .value
            .clone()
            .unwrap_or(Object::Undefined)
    }

    pub fn value(&self) -> Option<Object> {
        self.inner.borrow().value.clone()
    }

    pub fn inspect(&self) -> String {
        let g = self.inner.borrow();
        match g.state {
            PromiseState::Pending => "<promise pending>".to_string(),
            PromiseState::Fulfilled => match &g.value {
                Some(o) => format!("<promise resolved: {}>", o.inspect()),
                None => "<promise resolved>".to_string(),
            },
            PromiseState::Rejected => match &g.value {
                Some(o) => format!("<promise rejected: {}>", o.inspect()),
                None => "<promise rejected>".to_string(),
            },
        }
    }
}
