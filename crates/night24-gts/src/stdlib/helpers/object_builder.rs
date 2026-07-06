use super::*;

pub(crate) struct ObjectBuilder {
    hash: HashData,
}

impl ObjectBuilder {
    pub(crate) fn new() -> Self {
        Self {
            hash: HashData::default(),
        }
    }

    pub(crate) fn set(mut self, key: impl Into<String>, value: Object) -> Self {
        self.hash.set(key, value);
        self
    }

    pub(crate) fn insert(&mut self, key: impl Into<String>, value: Object) -> &mut Self {
        self.hash.set(key, value);
        self
    }

    pub(crate) fn into_shared(self) -> Rc<RefCell<HashData>> {
        Rc::new(RefCell::new(self.hash))
    }

    pub(crate) fn build(self) -> Object {
        Object::Hash(self.into_shared())
    }
}
