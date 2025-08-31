use std::fmt::{Debug, Formatter};

pub struct CachedValue<T> {
    value: Option<T>,
    getter: Box<dyn FnMut() -> T + Send>,
}

impl<T: Debug> Debug for CachedValue<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("CachedValue")
            .field(&self.value)
            .field(&"<closure>")
            .finish()
    }
}

impl<T> CachedValue<T> {
    pub fn new(getter: impl FnMut() -> T + Send + 'static) -> Self {
        Self {
            value: None,
            getter: Box::new(getter),
        }
    }

    pub fn get_cached(&mut self) -> &mut T {
        self.value.get_or_insert_with(&mut self.getter)
    }

    pub fn update(&mut self) {
        self.value = Some((self.getter)());
    }
}
