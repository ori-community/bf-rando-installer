use std::fmt::{Debug, Formatter};

pub struct CachedValue<T, D> {
    dependency: Option<D>,
    value: Option<T>,
    getter: Box<dyn FnMut(D) -> T + Send>,
}

impl<T: Debug, D: Debug> Debug for CachedValue<T, D> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("CachedValue")
            .field(&self.dependency)
            .field(&self.value)
            .field(&"<closure>")
            .finish()
    }
}

impl<T, D> CachedValue<T, D> {
    pub fn new(getter: impl FnMut(D) -> T + Send + 'static) -> Self {
        Self {
            dependency: None,
            value: None,
            getter: Box::new(getter),
        }
    }
}

impl<T, D: Eq + Clone> CachedValue<T, D> {
    pub fn get_cached(&mut self, dependency: D) -> &T {
        if self.value.is_none() || self.dependency.as_ref() != Some(&dependency) {
            self.value = Some((self.getter)(dependency.clone()));
            self.dependency = Some(dependency);
        }

        match &self.value {
            Some(v) => v,
            None => unreachable!(),
        }
    }

    pub fn update(&mut self, dependency: D) {
        self.value = Some((self.getter)(dependency.clone()));
        self.dependency = Some(dependency);
    }
}
