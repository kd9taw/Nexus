//! Mutation-tracked record storage for consistent reads in small chunks.
//! Contents remain private here so every write passes through DerefMut.
use super::QsoRecord;

/// A read token becomes obsolete before any mutable access to the vector. The
/// wrapper covers indexing, mutable slices and all Vec methods, including future
/// mutation sites. It does not change records, ADIF, reconciliation or saves.
/// A reader retains its Arc, so a later allocation cannot reuse that identity.
#[derive(Debug, Clone, Default)]
pub(super) struct Records {
    values: Vec<QsoRecord>,
    token: std::sync::Arc<()>,
}
impl Records {
    pub(super) fn read_token(&self) -> std::sync::Arc<()> {
        self.token.clone()
    }
}
impl From<Vec<QsoRecord>> for Records {
    fn from(values: Vec<QsoRecord>) -> Self {
        Self {
            values,
            ..Self::default()
        }
    }
}
impl std::ops::Deref for Records {
    type Target = Vec<QsoRecord>;
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}
impl std::ops::DerefMut for Records {
    fn deref_mut(&mut self) -> &mut Self::Target {
        // With no retained reader/clone there is no old identity to invalidate.
        // Ordinary logging therefore adds no token allocation per contact.
        if std::sync::Arc::strong_count(&self.token) > 1 {
            self.token = std::sync::Arc::new(());
        }
        &mut self.values
    }
}
impl<'a> IntoIterator for &'a Records {
    type Item = &'a QsoRecord;
    type IntoIter = std::slice::Iter<'a, QsoRecord>;
    fn into_iter(self) -> Self::IntoIter {
        self.values.iter()
    }
}
