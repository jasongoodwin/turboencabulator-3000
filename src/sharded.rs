// use std::sync::{Mutex, Arc};
// use std::collections::HashMap;
//
// type ShardedHashMap<K, V> = HashMap<u8,Mutex<HashMap<K, V>>>;
//
// const NUM_SHARDS: u8 = u8::MAX;
//
// struct ShardedMap<K: std::hash::Hasher, V> {
//     map: ShardedHashMap<K, V>,
// }
//
// impl ShardedMap<K, V> {
//     pub fn new() -> Arc<ShardedMap<K, V>> {
//         let mut map = HashMap::new();
//
//         for i in 0..=NUM_SHARDS {
//             map.insert(i, Mutex::new(HashMap::new()));
//         }
//
//         Arc::new(ShardedMap { map })
//     }
//
//     pub fn get_shard_async(self, key: K) -> () {
//         self.map.get(key).unwrap().lock()
//     }
// }