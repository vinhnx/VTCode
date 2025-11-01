//! Integration tests for quick-cache functionality

use serde_json::Value;
use vtcode_core::tools::cache::FileCache;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_quick_cache_file_operations() {
        let cache = FileCache::new(100);

        // Test cache miss
        let result = cache.get_file("nonexistent").await;
        assert!(result.is_none());

        // Test cache put and get
        let test_data = Value::String("test content".to_string());
        cache
            .put_file("test_key".to_string(), test_data.clone())
            .await;

        let cached_result = cache.get_file("test_key").await;
        assert!(cached_result.is_some());
        assert_eq!(cached_result.unwrap(), test_data);

        // Test cache statistics
        let stats = cache.stats().await;
        assert!(stats.hits >= 1);
        assert!(stats.entries >= 1);
    }

    #[tokio::test]
    async fn test_quick_cache_directory_operations() {
        let cache = FileCache::new(100);

        // Test directory cache miss
        let result = cache.get_directory("nonexistent").await;
        assert!(result.is_none());

        // Test directory cache put and get
        let test_data = Value::Array(vec![
            Value::String("file1.txt".to_string()),
            Value::String("file2.txt".to_string()),
        ]);
        cache
            .put_directory("test_dir".to_string(), test_data.clone())
            .await;

        let cached_result = cache.get_directory("test_dir").await;
        assert!(cached_result.is_some());
        assert_eq!(cached_result.unwrap(), test_data);
    }

    #[tokio::test]
    async fn test_quick_cache_capacity() {
        let cache = FileCache::new(10);

        let (file_capacity, dir_capacity) = cache.capacity();
        assert_eq!(file_capacity, 10);
        assert_eq!(dir_capacity, 5); // Half of file capacity
    }

    #[tokio::test]
    async fn test_quick_cache_clear() {
        let cache = FileCache::new(100);

        // Add some data
        let test_data = Value::String("test".to_string());
        cache.put_file("test_key".to_string(), test_data).await;
        cache
            .put_directory("test_dir".to_string(), Value::Null)
            .await;

        // Verify data exists
        assert!(cache.get_file("test_key").await.is_some());
        assert!(cache.get_directory("test_dir").await.is_some());

        // Clear cache
        cache.clear().await;

        // Verify data is gone
        assert!(cache.get_file("test_key").await.is_none());
        assert!(cache.get_directory("test_dir").await.is_none());

        // Check stats are reset
        let stats = cache.stats().await;
        assert_eq!(stats.entries, 0);
        assert_eq!(stats.total_size_bytes, 0);
    }
}
