use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::{StorageError, StorageResult};
use crate::models::{QueryFilter, Record, RecordKind};

pub struct MemoryStore {
    records: Arc<RwLock<HashMap<String, Record>>>,
    key_index: Arc<RwLock<HashMap<(String, String), String>>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            records: Arc::new(RwLock::new(HashMap::new())),
            key_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn insert(&self, record: Record) -> StorageResult<()> {
        let mut records = self.records.write().await;
        let mut key_index = self.key_index.write().await;
        
        let key = (record.kind.as_str().to_string(), record.key.clone());
        key_index.insert(key, record.id.clone());
        records.insert(record.id.clone(), record);
        
        Ok(())
    }

    pub async fn update(&self, record: Record) -> StorageResult<()> {
        let mut records = self.records.write().await;
        
        if !records.contains_key(&record.id) {
            return Err(StorageError::NotFound(record.id));
        }
        
        records.insert(record.id.clone(), record);
        Ok(())
    }

    pub async fn delete(&self, id: &str) -> StorageResult<()> {
        let mut records = self.records.write().await;
        let mut key_index = self.key_index.write().await;
        
        if let Some(record) = records.remove(id) {
            let key = (record.kind.as_str().to_string(), record.key);
            key_index.remove(&key);
            Ok(())
        } else {
            Err(StorageError::NotFound(id.to_string()))
        }
    }

    pub async fn get(&self, id: &str) -> StorageResult<Record> {
        let records = self.records.read().await;
        
        if let Some(record) = records.get(id) {
            Ok(record.clone())
        } else {
            Err(StorageError::NotFound(id.to_string()))
        }
    }

    pub async fn get_by_key(&self, kind: &RecordKind, key: &str) -> StorageResult<Option<Record>> {
        let key_index = self.key_index.read().await;
        
        if let Some(id) = key_index.get(&(kind.as_str().to_string(), key.to_string())) {
            let records = self.records.read().await;
            Ok(records.get(id).cloned())
        } else {
            Ok(None)
        }
    }

    pub async fn query(&self, filter: QueryFilter) -> StorageResult<Vec<Record>> {
        let records = self.records.read().await;
        
        let mut results: Vec<Record> = records.values()
            .filter(|r| {
                if let Some(kind) = &filter.kind {
                    if r.kind != *kind {
                        return false;
                    }
                }
                if let Some(prefix) = &filter.key_prefix {
                    if !r.key.starts_with(prefix) {
                        return false;
                    }
                }
                true
            })
            .cloned()
            .collect();
        
        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        
        let offset = filter.offset.unwrap_or(0);
        let limit = filter.limit.unwrap_or(100);
        
        results = results.into_iter().skip(offset).take(limit).collect();
        
        Ok(results)
    }

    pub async fn count(&self, kind: Option<&RecordKind>) -> StorageResult<usize> {
        let records = self.records.read().await;
        
        let count = records.values()
            .filter(|r| {
                if let Some(k) = kind {
                    r.kind == *k
                } else {
                    true
                }
            })
            .count();
        
        Ok(count)
    }

    pub async fn clear(&self, kind: Option<&RecordKind>) -> StorageResult<usize> {
        let mut records = self.records.write().await;
        let mut key_index = self.key_index.write().await;
        
        let to_remove: Vec<String> = records.values()
            .filter(|r| {
                if let Some(k) = kind {
                    r.kind == *k
                } else {
                    true
                }
            })
            .map(|r| r.id.clone())
            .collect();
        
        for id in &to_remove {
            if let Some(record) = records.remove(id) {
                let key = (record.kind.as_str().to_string(), record.key);
                key_index.remove(&key);
            }
        }
        
        Ok(to_remove.len())
    }
}

impl Default for MemoryStore {
    fn default() -> Self {
        Self::new()
    }
}
